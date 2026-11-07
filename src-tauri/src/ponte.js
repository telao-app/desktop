// Ponte entre o site e o app (Tauri). Roda antes do site, em toda página da janela principal.
// - window.telaoDesktop.teclas: atalhos e apertar para falar com o app em segundo plano (mesmo contrato do Electron).
// - getDisplayMedia: o vídeo vem do próprio webview (seletor do sistema); o som vem do nativo, como PCM int16
//   estéreo 48 kHz: no Mac o som do computador sem o que o próprio Telão toca, no Windows o som só do programa
//   da janela escolhida (tela inteira: o seletor do WebView2 já oferece o som do sistema).
// - window.Notification: a notificação do site sai como a do sistema; clicar abre a sala.
// - window.telaoDesktop.jogo: nome do jogo aberto ("Jogando X"), ou null.
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
    notificacoes: true, // o site liga as notificações uma vez nesta versão (antes no Mac elas nem existiam no app)
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
  const original = md?.getDisplayMedia?.bind(md);
  if (original) {
    md.getDisplayMedia = async (pedido = {}) => {
      if (!pedido.audio) return original(pedido);
      let stream, alvo;
      if (mac) {
        // o WebKit não entrega som de tela; pedir áudio a ele só atrapalha
        stream = await original({ ...pedido, audio: false });
        alvo = "sistema";
      } else {
        stream = await original(pedido);
        const hwnd = janelaDe(stream.getVideoTracks()[0]);
        if (!hwnd || stream.getAudioTracks().length) return stream;
        alvo = hwnd;
      }
      const ac = new AudioContext({ sampleRate: 48000 });
      await ac.audioWorklet.addModule(URL.createObjectURL(new Blob([worklet], { type: "text/javascript" })));
      const no = new AudioWorkletNode(ac, "telao-som", { outputChannelCount: [2] });
      const destino = ac.createMediaStreamDestination();
      no.connect(destino);
      try {
        await T.invoke("som", { alvo, pcm: canal((buf) => no.port.postMessage(buf, [buf])) });
      } catch (e) {
        // sem permissão de gravar o som (ou Windows antigo): a tela segue só com vídeo, como no navegador
        console.warn("som da transmissão:", e);
        ac.close();
        return stream;
      }
      stream.addTrack(destino.stream.getAudioTracks()[0]);
      // parar a transmissão (stop() da página ou o fim da captura) encerra o som nativo
      let parado = false;
      const parar = () => {
        if (parado) return;
        parado = true;
        T.invoke("som_parar");
        ac.close();
      };
      for (const t of stream.getTracks()) {
        const stop = t.stop.bind(t);
        t.stop = () => { stop(); parar(); };
        t.addEventListener("ended", parar);
      }
      return stream;
    };
  }

  addEventListener("keydown", async (e) => {
    if (!((mac ? e.metaKey : e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "d")) return;
    e.preventDefault();
    T.invoke("diagnostico", { texto: await transmissao().catch((err) => "Erro: " + err.message) });
  });

  // o que a minha tela está enviando e por que está limitada
  async function transmissao() {
    const pub = window.__voice?.room.localParticipant.getTrackPublication("screen_share");
    if (!pub?.track) return "Você não está transmitindo a tela.";
    const c = pub.track.mediaStreamTrack.getSettings();
    const linhas = ["Captura: " + c.width + "x" + c.height + " @ " + c.frameRate];
    (await pub.track.sender.getStats()).forEach((s) => {
      if (s.type !== "outbound-rtp" || s.kind !== "video") return;
      const d = s.qualityLimitationDurations || {};
      linhas.push("Enviando: " + s.frameWidth + "x" + s.frameHeight + " a " + s.framesPerSecond + " fps",
        "Limitado por: " + s.qualityLimitationReason + " (cpu " + Math.round(d.cpu || 0) + "s, banda " + Math.round(d.bandwidth || 0) + "s)",
        "Codificador: " + s.encoderImplementation + (s.powerEfficientEncoder ? " (placa)" : " (processador)"),
        "Codificação: " + (1000 * s.totalEncodeTime / s.framesEncoded).toFixed(1) + " ms/quadro",
        "Alvo: " + Math.round((s.targetBitrate || 0) / 1000) + " kbps");
    });
    return linhas.join("\n");
  }
})();
