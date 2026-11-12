// Ponte entre o site e o app (Tauri). Roda antes do site, em toda página da janela principal.
// - window.telaoDesktop.teclas: atalhos e apertar para falar com o app em segundo plano (mesmo contrato do Electron).
// - getDisplayMedia: no Windows, seletor próprio (janelas, jogos em tela cheia, telas) e captura pelo app (captura.rs):
//   cada quadro chega por memória compartilhada do WebView2 e vira a faixa de vídeo. No Mac (e no Windows, se a captura
//   própria falhar) o vídeo vem do próprio webview (seletor do sistema). O som vem do nativo, como PCM int16 estéreo
//   48 kHz: no Mac o som do computador sem o que o próprio Telão toca; no Windows o som só do programa da janela
//   escolhida, ou na tela inteira o do computador menos o do Telão.
// - window.Notification: a notificação do site sai como a do sistema; clicar abre a sala.
// - window.telaoDesktop.jogo: nome do jogo aberto ("Jogando X"), ou null.
// - window.telaoDesktop.musica: "Artista - Música" tocando no Spotify ("Ouvindo X"), ou null.
// - Ctrl+Shift+D (⌘⇧D no Mac): diagnóstico da transmissão.
(() => {
  const T = window.__TAURI_INTERNALS__;
  if (!T || window.telaoDesktop) return;
  const mac = /Mac/.test(navigator.platform);

  // Canal do Tauri: cada mensagem vem com um índice e pode chegar fora de ordem.
  const canal = (aoChegar) => {
    let proxima = 0;
    const fila = new Map();
    const id = T.transformCallback((m) => {
      fila.set(m.index, m);
      while (fila.has(proxima)) {
        const atual = fila.get(proxima);
        fila.delete(proxima++);
        if (!atual.end) aoChegar(atual.message);
      }
    });
    return `__CHANNEL__:${id}`;
  };

  window.telaoDesktop = {
    teclas: (t, aoApertar, aoFalar) =>
      T.invoke("teclas", {
        escolhidas: t,
        eventos: canal((e) => (e.atalho ? aoApertar(e.atalho) : aoFalar(e.falar))),
      }),
    jogo: () => T.invoke("jogo"),
    musica: () => T.invoke("musica"),
    abrirComPc: (ligar) => T.invoke("abrir_com_pc", { ligar: ligar ?? null }),
    notificacoes: true,
    atualizacao: () => T.invoke("atualizacao"), // versão nova pronta ("0.2.5") ou null
    atualizar: () => T.invoke("atualizar"), // instala e reabre, como o "Reiniciar agora" do aviso // o site liga as notificações uma vez nesta versão (antes no Mac elas nem existiam no app)
  };

  // Notificação do navegador vira a do sistema: o site segue usando `new Notification(...)` e o `onclick` dela (abrir a
  // sala), e o app mostra a nativa. No Mac o WebKit do app nem tem essa API. Permissão: o próprio sistema cuida.
  const abertas = new Map();
  let seq = 0;
  const cliques = canal((id) => abertas.get(id)?.onclick?.());
  window.Notification = class {
    static permission = "granted";
    static requestPermission = async () => "granted";
    constructor(titulo, o = {}) {
      this.onclick = null;
      this.id = ++seq;
      abertas.set(this.id, this);
      if (abertas.size > 50) abertas.delete(abertas.keys().next().value);
      T.invoke("notificar", { id: this.id, titulo, corpo: o.body ?? "", cliques }).catch((e) => console.warn("notificação:", e));
    }
    close() {
      abertas.delete(this.id);
    }
  };

  // toca o PCM que chega do nativo; se acumular atraso, descarta o excesso
  const worklet = `registerProcessor('telao-som', class extends AudioWorkletProcessor {
    constructor() {
      super(); this.fila = []; this.pos = 0;
      this.port.onmessage = (e) => { this.fila.push(new Int16Array(e.data)); if (this.fila.length > 40) this.fila.splice(0, this.fila.length - 10); };
    }
    process(_entrada, [[esq, dir]]) {
      for (let i = 0; i < esq.length; i++) {
        const pcm = this.fila[0];
        if (!pcm) { esq[i] = dir[i] = 0; continue; }
        esq[i] = pcm[this.pos] / 32768; dir[i] = pcm[this.pos + 1] / 32768;
        if ((this.pos += 2) >= pcm.length) { this.fila.shift(); this.pos = 0; }
      }
      return true;
    }
  });`;

  // Windows: o Chromium do WebView2 rotula a faixa de janela como "window:<hwnd>:0"
  const janelaDe = (faixa) => /^window:(\d+):/.exec(faixa.label)?.[1] ?? /^window:(\d+):/.exec(faixa.getSettings().deviceId ?? "")?.[1];

  const md = navigator.mediaDevices;
  const nativo = md?.getDisplayMedia?.bind(md);
  let relato = ""; // como foi a última captura (vai para o diagnóstico)
  let capturando = false; // captura própria no ar

  // Som do nativo virando faixa de áudio. alvo: hwnd da janela, ou "sistema". null se não deu (sem permissão, Windows
  // antigo): a tela segue só com vídeo, como no navegador.
  async function somNativo(alvo) {
    const ac = new AudioContext({ sampleRate: 48000 });
    try {
      await ac.audioWorklet.addModule(URL.createObjectURL(new Blob([worklet], { type: "text/javascript" })));
      const no = new AudioWorkletNode(ac, "telao-som", { outputChannelCount: [2] });
      const destino = ac.createMediaStreamDestination();
      no.connect(destino);
      await T.invoke("som", { alvo, pcm: canal((buf) => no.port.postMessage(buf, [buf])) });
      return { faixa: destino.stream.getAudioTracks()[0], parar: () => { T.invoke("som_parar"); ac.close(); } };
    } catch (e) {
      console.warn("som da transmissão:", e);
      ac.close();
      return null;
    }
  }

  // parar a transmissão (stop() da página ou o fim da captura) encerra o que o nativo estiver mandando
  function aoParar(stream, parar) {
    let parado = false;
    const uma = () => { if (!parado) { parado = true; parar(); } };
    for (const t of stream.getTracks()) {
      const stop = t.stop.bind(t);
      t.stop = () => { stop(); uma(); };
      t.addEventListener("ended", uma);
    }
  }

  // seletor do sistema (Mac; Windows quando a captura própria não deu)
  async function peloSistema(pedido) {
    if (!pedido.audio) {
      const s = await nativo(pedido);
      if (!mac) T.invoke("esconder_barra").catch(() => {});
      return s;
    }
    let stream, alvo;
    if (mac) {
      // o WebKit não entrega som de tela; pedir áudio a ele só atrapalha
      stream = await nativo({ ...pedido, audio: false });
      alvo = "sistema";
    } else {
      stream = await nativo(pedido);
      // Windows: some a barra "<site> está compartilhando uma janela" do WebView2 (o Telão já mostra e tem o botão de parar)
      T.invoke("esconder_barra").catch(() => {});
      const hwnd = janelaDe(stream.getVideoTracks()[0]);
      if (!hwnd || stream.getAudioTracks().length) return stream;
      alvo = hwnd;
    }
    const som = await somNativo(alvo);
    if (!som) return stream;
    stream.addTrack(som.faixa);
    aoParar(stream, som.parar);
    return stream;
  }

  // ---------- Windows: captura própria ----------

  // cada quadro: o app escreve na memória compartilhada e avisa; a página copia para um VideoFrame e devolve o buffer
  // (primeiro i32 = 0) para o app usar de novo
  const recebedores = new Map(); // sessão -> (dados, buffer) => void
  window.chrome?.webview?.addEventListener?.("sharedbufferreceived", (e) => {
    const d = e.additionalData || {};
    const buf = e.getBuffer();
    try {
      recebedores.get(d.s)?.(d, buf);
    } catch (err) {
      console.warn("quadro:", err);
    } finally {
      new Int32Array(buf, 0, 1)[0] = 0;
      window.chrome.webview.releaseBuffer(buf);
    }
  });
  const proprio = !mac && !!window.chrome?.webview?.releaseBuffer && typeof VideoFrame === "function";

  // faixa de vídeo que recebe os quadros; o tamanho real aparece no getSettings (o site e o LiveKit leem dali)
  function faixaDeVideo(fonte, fps) {
    let largura = fonte.largura || 1920;
    let altura = fonte.altura || 1080;
    let faixa, escrever, fechar;
    if (typeof MediaStreamTrackGenerator === "function") {
      const gerador = new MediaStreamTrackGenerator({ kind: "video" });
      const escritor = gerador.writable.getWriter();
      let pendentes = 0;
      escrever = (q) => {
        if (pendentes > 2) return q.close(); // o codificador está atrasado: pula este
        pendentes++;
        escritor.write(q).then(() => pendentes--, () => { pendentes--; q.close(); });
      };
      fechar = () => escritor.close().catch(() => {});
      faixa = gerador;
    } else {
      // sem o gerador: desenha num canvas e transmite o canvas
      const tela = document.createElement("canvas");
      tela.width = largura;
      tela.height = altura;
      const ctx = tela.getContext("2d", { alpha: false, desynchronized: true });
      faixa = tela.captureStream(0).getVideoTracks()[0];
      escrever = (q) => {
        if (tela.width !== q.codedWidth || tela.height !== q.codedHeight) [tela.width, tela.height] = [q.codedWidth, q.codedHeight];
        ctx.drawImage(q, 0, 0);
        q.close();
        faixa.requestFrame();
      };
      fechar = () => {};
    }
    const configuracao = faixa.getSettings.bind(faixa);
    const superficie = fonte.tipo === "tela" ? "monitor" : "window";
    faixa.getSettings = () => ({
      ...configuracao(),
      width: largura,
      height: altura,
      aspectRatio: largura / altura,
      frameRate: fps,
      displaySurface: superficie,
      logicalSurface: true,
      cursor: "always",
      deviceId: fonte.id,
    });
    faixa.applyConstraints = async () => {}; // o tamanho e os quadros quem decide é a captura
    Object.defineProperty(faixa, "label", { value: `${fonte.id}:${fonte.nome}` });
    const quadro = (d, buf) => {
      largura = d.w;
      altura = d.h;
      escrever(new VideoFrame(new Uint8Array(buf, d.o, d.w * d.h * 4), { format: "BGRX", codedWidth: d.w, codedHeight: d.h, timestamp: d.t }));
    };
    return { faixa, quadro, fechar };
  }

  async function capturaPropria(pedido) {
    const escolha = await seletorProprio(pedido);
    if (escolha.sistema) {
      // a pessoa preferiu o seletor do WebView2 (o caminho de antes da captura própria)
      const stream = await peloSistema(pedido);
      relato = "seletor do Windows (escolhido no seletor do Telão)";
      return stream;
    }
    const fonte = escolha.fonte;
    const pedidoFps = typeof pedido.video === "object" ? pedido.video.frameRate : null;
    const fps = Math.round(Math.min(120, Math.max(5, Number(pedidoFps?.max ?? pedidoFps?.ideal ?? pedidoFps) || 30)));
    const v = faixaDeVideo(fonte, fps);
    let sessao = null;
    let acabou = false;
    const fim = () => {
      // a janela fechou: a faixa termina como termina a do navegador (o site tira a transmissão)
      acabou = true;
      v.fechar();
      setTimeout(() => {
        if (v.faixa.readyState === "live") {
          v.faixa.stop();
          v.faixa.dispatchEvent(new Event("ended"));
        }
      }, 100);
    };
    sessao = await T.invoke("captura", { id: fonte.id, fps, eventos: canal((m) => m === "fim" && fim()) });
    recebedores.set(sessao, v.quadro);
    capturando = true;
    relato = `captura própria: ${fonte.jogo ? "jogo " + fonte.jogo : fonte.nome} (${fonte.id}), ${fps} fps pedidos` +
      (fonte.minimizada ? ", estava minimizada" : "") + (typeof MediaStreamTrackGenerator === "function" ? "" : ", pelo canvas");
    const stream = new MediaStream([v.faixa]);
    const som = escolha.som ? await somNativo(fonte.tipo === "tela" ? "sistema" : fonte.id.split(":")[1]) : null;
    if (som) stream.addTrack(som.faixa);
    aoParar(stream, () => {
      recebedores.delete(sessao);
      capturando = false;
      T.invoke("captura_parar", { sessao }).catch(() => {});
      som?.parar();
      if (!acabou) v.fechar();
    });
    return stream;
  }

  if (nativo) {
    md.getDisplayMedia = async (pedido = {}) => {
      if (proprio) {
        try {
          return await capturaPropria(pedido);
        } catch (e) {
          if (e?.name === "NotAllowedError") throw e; // a pessoa cancelou
          console.warn("captura própria:", e);
          relato = "captura própria falhou (" + (e?.message || e) + "), foi pelo seletor do WebView2";
        }
      }
      return peloSistema(pedido);
    };
  }

  // ---------- Windows: o seletor ----------

  const CSS = `
    :host { all: initial; }
    * { box-sizing: border-box; }
    .fundo { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center; padding: 16px;
      background: rgba(8, 6, 14, .62); backdrop-filter: blur(3px); color: #efeaf7; font: 14px/1.4 var(--fonte);
      animation: surge .14s ease-out; }
    .caixa { width: min(940px, 100%); height: min(660px, 100%); display: flex; flex-direction: column;
      background: #231c31; border: 1px solid #3a3050; border-radius: 18px; box-shadow: 0 24px 70px rgba(0, 0, 0, .55);
      animation: sobe .16s ease-out; outline: none; }
    header { display: flex; align-items: center; justify-content: space-between; padding: 18px 20px 4px 22px; }
    h2 { margin: 0; font-size: 18px; font-weight: 700; }
    .x { border: 0; background: transparent; color: #b6abc8; width: 32px; height: 32px; border-radius: 50%; font-size: 22px;
      line-height: 1; cursor: pointer; }
    .x:hover { background: #33294a; color: #fff; }
    nav { display: flex; gap: 6px; padding: 10px 22px 12px; }
    .aba { border: 0; background: transparent; color: #b6abc8; padding: 7px 14px; border-radius: 999px; font: inherit;
      font-weight: 600; cursor: pointer; }
    .aba:hover { color: #fff; }
    .aba[aria-selected="true"] { background: #3a2f53; color: #fff; }
    .aba small { opacity: .6; font-weight: 500; margin-left: 5px; }
    .grade { flex: 1; min-height: 220px; overflow: auto; padding: 4px 22px 20px; display: grid; gap: 14px;
      grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); align-content: start; }
    .cartao { all: unset; box-sizing: border-box; cursor: pointer; display: flex; flex-direction: column; gap: 7px; padding: 8px;
      border-radius: 14px; border: 2px solid transparent; background: #2c2440; transition: background .12s, border-color .12s; }
    .cartao:hover { background: #342a4b; }
    .cartao:focus-visible { outline: 2px solid #f7d046; outline-offset: 2px; }
    .cartao[aria-selected="true"] { border-color: #f7d046; background: #3a2f53; }
    .mini { position: relative; aspect-ratio: 16 / 9; border-radius: 9px; overflow: hidden; background: #161120;
      display: flex; align-items: center; justify-content: center; }
    .mini canvas { width: 100%; height: 100%; object-fit: contain; display: block; }
    .vazio { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center;
      gap: 3px; padding: 10px 12px; text-align: center; color: #a69cba; font-size: 12px; background: #161120; }
    [hidden] { display: none !important; }
    .vazio b { color: #efeaf7; font-size: 15px; font-weight: 700; }
    .selo { position: absolute; top: 7px; left: 7px; padding: 2px 8px; border-radius: 999px; background: #f7d046;
      color: #231c31; font-size: 11px; font-weight: 800; letter-spacing: .02em; }
    .nome { padding: 0 2px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .sub { padding: 0 2px; margin-top: -5px; color: #a69cba; font-size: 12px; white-space: nowrap; overflow: hidden;
      text-overflow: ellipsis; }
    .nada { grid-column: 1 / -1; padding: 48px 0; text-align: center; color: #a69cba; }
    footer { display: flex; align-items: center; gap: 12px; padding: 14px 20px 14px 22px; border-top: 1px solid #3a3050; }
    .som { display: flex; align-items: center; gap: 8px; color: #d8d0e6; cursor: pointer; user-select: none; }
    .som input { width: 16px; height: 16px; margin: 0; accent-color: #f7d046; cursor: pointer; }
    .outro { border: 0; background: transparent; padding: 4px 2px; color: #a69cba; font: inherit; font-size: 13px;
      text-decoration: underline; text-underline-offset: 3px; cursor: pointer; }
    .outro:hover { color: #efeaf7; }
    .acoes { display: flex; gap: 10px; margin-left: auto; }
    .acoes button { border: 0; border-radius: 999px; padding: 10px 20px; font: inherit; font-weight: 700; cursor: pointer; }
    .cancelar { background: #3a2f53; color: #efeaf7; }
    .cancelar:hover { background: #45385f; }
    .ok { background: #f7d046; color: #231c31; }
    .ok:hover { background: #ffdd5c; }
    .ok:disabled { opacity: .4; cursor: default; }
    @keyframes surge { from { opacity: 0; } }
    @keyframes sobe { from { transform: translateY(8px) scale(.985); } }
    @media (prefers-reduced-motion: reduce) { .fundo, .caixa { animation: none; } }
  `;

  // miniatura que o app manda: 8 bytes (largura, altura) e RGBA
  async function desenharMiniatura(id, tela) {
    const buf = await T.invoke("miniatura", { id }).catch(() => null);
    if (!(buf instanceof ArrayBuffer) || buf.byteLength < 8) return false;
    const [w, h] = new Uint32Array(buf, 0, 2);
    if (!w || !h || buf.byteLength < 8 + w * h * 4) return false;
    tela.width = w;
    tela.height = h;
    tela.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h), 0, 0);
    return true;
  }

  // Lista as janelas abertas (jogo em tela cheia também, mesmo minimizado) e as telas, com miniatura, e devolve a
  // escolhida. Cancelar rejeita como o seletor do navegador (NotAllowedError).
  function seletorProprio(pedido) {
    return new Promise((resolve, reject) => {
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;inset:0;z-index:2147483647";
      const raiz = host.attachShadow({ mode: "closed" });
      raiz.innerHTML = `<style>${CSS}</style>
        <div class="fundo">
          <div class="caixa" role="dialog" aria-modal="true" aria-labelledby="t" tabindex="-1">
            <header><h2 id="t">Mostrar minha tela</h2><button class="x" aria-label="Fechar">×</button></header>
            <nav role="tablist">
              <button class="aba" role="tab" data-aba="janela">Aplicativos<small></small></button>
              <button class="aba" role="tab" data-aba="tela">Telas inteiras<small></small></button>
            </nav>
            <div class="grade" role="listbox" aria-label="O que mostrar"></div>
            <footer>
              <label class="som"><input type="checkbox"> Transmitir o som</label>
              <button class="outro" title="O seletor do WebView2, como antes da 0.2.13 (não mostra jogo em tela cheia)">Usar o seletor do Windows</button>
              <div class="acoes"><button class="cancelar">Cancelar</button><button class="ok" disabled>Mostrar</button></div>
            </footer>
          </div>
        </div>`;
      const $ = (s) => raiz.querySelector(s);
      $(".fundo").style.setProperty("--fonte", getComputedStyle(document.body).fontFamily || "system-ui, sans-serif");
      const grade = $(".grade");
      const ok = $(".ok");
      const som = $(".som input");
      som.checked = !!pedido.audio;
      $(".som").hidden = !pedido.audio;

      let fontes = [];
      let aba = typeof pedido.video === "object" && pedido.video.displaySurface === "monitor" ? "tela" : "janela";
      let escolhida = null;
      const cartoes = new Map(); // id -> { el, tela, fonte, desenhada }
      let fila = [];
      let ocupados = 0;
      let aberto = true;
      const anterior = document.activeElement;

      const atualizarMiniaturas = () => {
        while (aberto && ocupados < 3 && fila.length) {
          const c = cartoes.get(fila.shift());
          if (!c || c.fonte.minimizada || c.fonte.tipo !== aba) continue;
          ocupados++;
          desenharMiniatura(c.fonte.id, c.tela).then((deu) => {
            c.desenhada ||= deu;
            c.el.querySelector(".vazio").hidden = c.desenhada;
          }).finally(() => { ocupados--; atualizarMiniaturas(); });
        }
      };

      const cartao = (f) => {
        const el = document.createElement("button");
        el.className = "cartao";
        el.setAttribute("role", "option");
        el.innerHTML = `<div class="mini"><canvas></canvas><div class="vazio"><b></b><span></span></div></div><div class="nome"></div><div class="sub"></div>`;
        el.addEventListener("click", () => escolher(f.id));
        el.addEventListener("dblclick", () => { escolher(f.id); confirmar(); });
        const c = { el, tela: el.querySelector("canvas"), fonte: f, desenhada: false };
        cartoes.set(f.id, c);
        return c;
      };

      const preencher = (c, f) => {
        c.fonte = f;
        const el = c.el;
        el.querySelector(".nome").textContent = f.jogo || f.nome;
        el.querySelector(".nome").title = f.nome;
        el.querySelector(".sub").textContent = f.tipo === "tela"
          ? f.programa + (f.principal ? " · principal" : "")
          : f.jogo ? (f.minimizada ? "em tela cheia" : f.nome) : f.programa;
        const vazio = el.querySelector(".vazio");
        vazio.querySelector("b").textContent = f.jogo || (f.minimizada ? "Minimizada" : "");
        vazio.querySelector("span").textContent = f.minimizada
          ? (f.jogo ? "A imagem aparece quando você voltar pro jogo" : "A imagem aparece quando a janela voltar")
          : "carregando…";
        vazio.hidden = c.desenhada && !f.minimizada;
        let selo = el.querySelector(".selo");
        if (f.jogo && !selo) {
          selo = document.createElement("span");
          selo.className = "selo";
          selo.textContent = "JOGO";
          el.querySelector(".mini").append(selo);
        } else if (!f.jogo) selo?.remove();
      };

      const desenhar = () => {
        for (const b of raiz.querySelectorAll(".aba")) {
          b.setAttribute("aria-selected", String(b.dataset.aba === aba));
          b.querySelector("small").textContent = fontes.filter((f) => f.tipo === b.dataset.aba).length || "";
        }
        const daAba = fontes.filter((f) => f.tipo === aba);
        grade.replaceChildren(...daAba.map((f) => cartoes.get(f.id).el));
        if (!daAba.length) {
          const nada = document.createElement("div");
          nada.className = "nada";
          nada.textContent = aba === "tela" ? "Nenhuma tela encontrada." : "Nenhum aplicativo aberto.";
          grade.append(nada);
        }
        for (const c of cartoes.values()) c.el.setAttribute("aria-selected", String(c.fonte.id === escolhida));
        ok.disabled = !fontes.some((f) => f.id === escolhida && f.tipo === aba);
      };

      const listar = async () => {
        const novas = await T.invoke("fontes").catch(() => null);
        if (!aberto) return;
        if (!novas) return fontes.length ? null : sair(new Error("não deu para listar as janelas"));
        const primeira = !fontes.length;
        for (const f of novas) preencher(cartoes.get(f.id) ?? cartao(f), f);
        for (const id of [...cartoes.keys()]) if (!novas.some((f) => f.id === id)) cartoes.delete(id);
        fontes = novas;
        if (primeira) {
          // jogo aberto: já vem escolhido; senão a tela principal na aba de telas
          const jogo = fontes.find((f) => f.jogo);
          if (jogo) [aba, escolhida] = ["janela", jogo.id];
          else if (aba === "tela") escolhida = fontes.find((f) => f.principal)?.id ?? null;
        }
        if (escolhida && !cartoes.has(escolhida)) escolhida = null;
        desenhar();
        fila = fontes.filter((f) => f.tipo === aba).map((f) => f.id);
        atualizarMiniaturas();
      };

      const escolher = (id) => {
        escolhida = id;
        desenhar();
      };
      const sair = (erro, valor) => {
        if (!aberto) return;
        aberto = false;
        clearInterval(relogio);
        removeEventListener("keydown", teclas, true);
        host.remove();
        anterior?.focus?.();
        erro ? reject(erro) : resolve(valor);
      };
      const cancelar = () => sair(new DOMException("A pessoa cancelou", "NotAllowedError"));
      const confirmar = () => {
        const fonte = fontes.find((f) => f.id === escolhida && f.tipo === aba);
        if (fonte) sair(null, { fonte, som: !!pedido.audio && som.checked });
      };
      const teclas = (e) => {
        if (e.key === "Escape") cancelar();
        else if (e.key === "Enter" && !ok.disabled && !e.target?.closest?.("button")) confirmar();
        else return;
        e.preventDefault();
        e.stopPropagation();
      };

      for (const b of raiz.querySelectorAll(".aba")) {
        b.addEventListener("click", () => {
          aba = b.dataset.aba;
          if (aba === "tela" && !fontes.some((f) => f.id === escolhida && f.tipo === "tela")) escolhida = fontes.find((f) => f.principal)?.id ?? null;
          desenhar();
          fila = fontes.filter((f) => f.tipo === aba).map((f) => f.id);
          atualizarMiniaturas();
        });
      }
      $(".x").addEventListener("click", cancelar);
      $(".cancelar").addEventListener("click", cancelar);
      $(".outro").addEventListener("click", () => sair(null, { sistema: true }));
      ok.addEventListener("click", confirmar);
      $(".fundo").addEventListener("mousedown", (e) => e.target === e.currentTarget && cancelar());
      addEventListener("keydown", teclas, true);
      document.documentElement.append(host);
      $(".caixa").focus();
      listar();
      const relogio = setInterval(listar, 3000); // janela abriu ou fechou; miniaturas novas
    });
  }

  addEventListener("keydown", async (e) => {
    if (!((mac ? e.metaKey : e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "d")) return;
    e.preventDefault();
    let texto = await transmissao().catch((err) => "Erro: " + err.message);
    if (relato) texto += "\n\n" + relato;
    if (capturando) texto += "\n" + (await T.invoke("captura_estado").catch((err) => "estado: " + err));
    const vendo = await assistindo().catch((err) => "Assistindo: erro " + err.message);
    if (vendo) texto += "\n\n" + vendo;
    texto += "\n\n" + (await placa().catch((err) => "Placa: erro " + err.message));
    T.invoke("diagnostico", { texto });
  });

  // Quem codificou a tela ao longo da transmissão: mostra se começou na placa e caiu para o processador, e quando.
  let faixaVista = null, inicio = 0, ultimo = "", trocas = [];
  setInterval(async () => {
    const faixa = window.__voice?.room?.localParticipant?.getTrackPublication("screen_share")?.track;
    if (!faixa?.sender) return;
    if (faixa !== faixaVista) [faixaVista, inicio, ultimo, trocas] = [faixa, Date.now(), "", []];
    const stats = await faixa.sender.getStats().catch(() => null);
    stats?.forEach((s) => {
      if (s.type !== "outbound-rtp" || s.kind !== "video" || !s.encoderImplementation) return;
      const agora = s.encoderImplementation + (s.powerEfficientEncoder ? " (placa)" : " (processador)");
      if (agora === ultimo || trocas.length >= 12) return;
      ultimo = agora;
      trocas.push(Math.round((Date.now() - inicio) / 1000) + " s: " + agora + " em " + s.frameWidth + "x" + s.frameHeight);
    });
  }, 2000);

  // Por que a placa não codifica (Windows): qual placa o app usa, se o codificador H.264 dela aceita 60 e 120 fps e,
  // na chamada, quem codifica um vídeo de teste local (fora da chamada o navegador esconde o nome do codificador).
  let bancadas = null;
  async function placa() {
    const gl = document.createElement("canvas").getContext("webgl");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    const linhas = ["Placa do app: " + (info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "?")];
    for (const fps of [60, 120]) {
      const aceita = await VideoEncoder.isConfigSupported({ codec: "avc1.420034", width: 1920, height: 1080, framerate: fps, bitrate: 8e6, hardwareAcceleration: "prefer-hardware", latencyMode: "realtime" })
        .then((r) => (r.supported ? "aceita" : "recusa"), (e) => "erro " + e.message);
      linhas.push("Codificador H.264 da placa, 1080p a " + fps + " fps: " + aceita);
    }
    if (!mac && window.__voice?.room) {
      bancadas ||= (async () => "Teste local 1080p: 60 fps " + (await bancada(60)) + " · 120 fps " + (await bancada(120)))().catch((e) => "Teste local: erro " + e.message);
      linhas.push(await bancadas);
    }
    return linhas.join("\n");
  }

  // Manda um canvas 1080p em H.264 para uma conexão local (sem servidor, sem E2EE) por 2 s e vê quem codificou.
  async function bancada(fps) {
    const tela = Object.assign(document.createElement("canvas"), { width: 1920, height: 1080 });
    const ctx = tela.getContext("2d");
    const a = new RTCPeerConnection(), b = new RTCPeerConnection();
    a.onicecandidate = (e) => e.candidate && b.addIceCandidate(e.candidate);
    b.onicecandidate = (e) => e.candidate && a.addIceCandidate(e.candidate);
    const faixa = tela.captureStream(fps).getVideoTracks()[0];
    const t = a.addTransceiver(faixa, { direction: "sendonly", sendEncodings: [{ maxFramerate: fps, maxBitrate: 8e6 }] });
    // o perfil que o site faz o Chromium enviar na chamada (42001f; o 42e01f só sai pelo processador)
    const h264 = RTCRtpSender.getCapabilities("video").codecs.filter((c) => /h264/i.test(c.mimeType));
    const daChamada = (c) => /42001f/.test(c.sdpFmtpLine) && /packetization-mode=1/.test(c.sdpFmtpLine);
    t.setCodecPreferences([...h264.filter(daChamada), ...h264.filter((c) => !daChamada(c))]);
    await a.setLocalDescription();
    await b.setRemoteDescription(a.localDescription);
    await b.setLocalDescription();
    await a.setRemoteDescription(b.localDescription);
    const pintar = setInterval(() => {
      ctx.fillStyle = `hsl(${Math.random() * 360} 50% 50%)`;
      ctx.fillRect(0, 0, 1920, 1080);
    }, 1000 / fps);
    await new Promise((r) => setTimeout(r, 2000));
    let quem = "sem resposta";
    (await t.sender.getStats()).forEach((s) => {
      if (s.type === "outbound-rtp" && s.encoderImplementation) quem = s.encoderImplementation + (s.powerEfficientEncoder ? " (placa)" : " (processador)");
    });
    clearInterval(pintar);
    faixa.stop();
    a.close();
    b.close();
    return quem;
  }

  // erros de criptografia (E2EE) por pessoa: quadro que não abre vira tela preta
  const errosCripto = new Map();
  let salaVista = null;
  setInterval(() => {
    const sala = window.__voice?.room;
    if (!sala?.on || sala === salaVista) return;
    salaVista = sala;
    sala.on("encryptionError", (err, p) => {
      const quem = p?.name || p?.identity || "?";
      const e = errosCripto.get(quem) || { n: 0 };
      errosCripto.set(quem, { n: e.n + 1, ultimo: err?.message || String(err) });
    });
  }, 2000);

  // o que a minha tela está enviando, por que está limitada e o que o servidor diz que recebe
  async function transmissao() {
    const pub = window.__voice?.room.localParticipant.getTrackPublication("screen_share");
    if (!pub?.track) return "Você não está transmitindo a tela.";
    const c = pub.track.mediaStreamTrack.getSettings();
    const linhas = ["Captura: " + c.width + "x" + c.height + " @ " + c.frameRate];
    const stats = await pub.track.sender.getStats();
    let parId = null;
    stats.forEach((s) => {
      if (s.type === "outbound-rtp" && s.kind === "video") {
        const d = s.qualityLimitationDurations || {};
        linhas.push("Enviando: " + s.frameWidth + "x" + s.frameHeight + " a " + s.framesPerSecond + " fps",
          "Limitado por: " + s.qualityLimitationReason + " (cpu " + Math.round(d.cpu || 0) + "s, banda " + Math.round(d.bandwidth || 0) + "s)",
          "Codificador: " + s.encoderImplementation + (s.powerEfficientEncoder ? " (placa)" : " (processador)"),
          "Codec: " + (stats.get(s.codecId)?.sdpFmtpLine || "?") + " · trocas de resolução: " + s.qualityLimitationResolutionChanges,
          "Codificação: " + (1000 * s.totalEncodeTime / s.framesEncoded).toFixed(1) + " ms/quadro",
          "Alvo: " + Math.round((s.targetBitrate || 0) / 1000) + " kbps",
          "Quadros-chave: " + s.keyFramesEncoded + " de " + s.framesEncoded + " · pedidos: PLI " + s.pliCount + ", FIR " + s.firCount + ", NACK " + s.nackCount,
          "Pacotes: " + s.packetsSent + " enviados, " + s.retransmittedPacketsSent + " reenviados");
      } else if (s.type === "remote-inbound-rtp" && s.kind === "video") {
        linhas.push("O servidor recebe: " + s.packetsLost + " pacotes perdidos (" + ((s.fractionLost || 0) * 100).toFixed(1) + "% agora), ida e volta " + Math.round((s.roundTripTime || 0) * 1000) + " ms");
      } else if (s.type === "transport" && s.selectedCandidatePairId) {
        parId = s.selectedCandidatePairId;
      }
    });
    const par = parId && stats.get(parId);
    if (par) {
      const l = stats.get(par.localCandidateId) || {};
      linhas.push("Conexão: " + l.candidateType + " " + l.protocol + (l.relayProtocol ? " (relay " + l.relayProtocol + ")" : "") +
        " · banda de saída estimada " + Math.round((par.availableOutgoingBitrate || 0) / 1000) + " kbps");
    }
    if (trocas.length) linhas.push("Codificadores: " + trocas.join(" → "));
    return linhas.join("\n");
  }

  // as telas dos outros que eu estou assistindo: o que chega e o que dá para mostrar
  async function assistindo() {
    const sala = window.__voice?.room;
    if (!sala) return "";
    const linhas = [];
    for (const p of sala.remoteParticipants.values()) {
      const pub = p.getTrackPublication("screen_share");
      const quem = p.name || p.identity;
      if (!pub?.track?.receiver) continue;
      (await pub.track.receiver.getStats()).forEach((s) => {
        if (s.type !== "inbound-rtp" || s.kind !== "video") return;
        linhas.push("Assistindo " + quem + ": " + s.frameWidth + "x" + s.frameHeight + " a " + s.framesPerSecond + " fps (" + pub.track.streamState + ")",
          "  quadros: " + s.framesReceived + " recebidos, " + s.framesDecoded + " decodificados, " + s.framesDropped + " descartados, " + s.keyFramesDecoded + " chave",
          "  pacotes: " + s.packetsReceived + " recebidos, " + s.packetsLost + " perdidos · pedidos PLI " + s.pliCount + ", NACK " + s.nackCount,
          "  travadas: " + s.freezeCount + " (" + (s.totalFreezesDuration || 0).toFixed(1) + " s) · decodificador " + s.decoderImplementation);
      });
    }
    for (const [quem, e] of errosCripto) linhas.push("Erros de criptografia de " + quem + ": " + e.n + " (último: " + e.ultimo + ")");
    return linhas.join("\n");
  }
})();
