const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile, spawn } = require('node:child_process');
const { app, BrowserWindow, clipboard, desktopCapturer, dialog, ipcMain, nativeImage, session, shell, systemPreferences } = require('electron');
const { autoUpdater } = require('electron-updater');

// ponytail: carrega o site; quando o front for empacotado aqui dentro, deixa de depender do servidor entregar o código
const URL_TELAO = process.env.TELAO_URL || 'https://telao.nyxdev.com.br';

// O site mostra a versão em "Sobre" lendo do user agent
app.userAgentFallback += ` Telao/${app.getVersion()}`;

app.whenReady().then(async () => {
  if (process.platform === 'darwin') {
    await systemPreferences.askForMediaAccess('microphone');
    await systemPreferences.askForMediaAccess('camera');
  }
  // Windows: o preload abre o seletor antes do getDisplayMedia para saber se é janela (som só do programa dela)
  ipcMain.handle('escolher-fonte', async () => {
    escolhida = await escolher();
    return escolhida && { id: escolhida.id, janela: escolhida.id.startsWith('window:') };
  });
  ipcMain.on('som-janela', (e, id) => somDaJanela(e.sender, id));
  ipcMain.handle('teclas', (_e, t) => teclas(t));

  session.defaultSession.setDisplayMediaRequestHandler(async (req, callback) => {
    const escolha = escolhida ?? (await escolher());
    escolhida = null;
    if (!escolha) return callback(null);
    // som do sistema só se a página pediu (entregar sem pedir, ou não entregar o pedido, quebra a captura).
    // No Mac só funciona com o app aberto por conta própria: aberto por terminal, o macOS dá a permissão ao terminal.
    // ponytail: Linux sem som do sistema; dá para capturar o monitor do PipeWire como microfone
    const som = req.audioRequested && process.platform !== 'linux';
    callback({ video: { id: escolha.id, name: escolha.name }, audio: som ? 'loopback' : undefined });
  });

  janela = new BrowserWindow({
    width: 1280, height: 800, autoHideMenuBar: true, backgroundColor: '#16191A',
    webPreferences: { preload: path.join(__dirname, 'preload.js') },
  });
  janela.loadURL(URL_TELAO);
  // Não lidas no ícone (Dock, barra de tarefas): o site já põe "(n) Telão" no título
  janela.webContents.on('page-title-updated', (_e, titulo) => contador(Number(titulo.match(/^\((\d+)\)/)?.[1] ?? 0)));
  // Ctrl+Shift+D (⌘⇧D no Mac): o que a minha tela está enviando e por que está limitada
  janela.webContents.on('before-input-event', (_e, t) => {
    if (t.type === 'keyDown' && (t.control || t.meta) && t.shift && t.key.toLowerCase() === 'd') diagnostico();
  });
  if (app.isPackaged) atualizar();
});

let janela;
let escolhida = null;
let captura = null;
const nativo = path.join(process.resourcesPath, 'telao-som.exe');

// Mac e Linux mostram o número; o Windows só aceita um ícone sobreposto, então vai uma bolinha vermelha
function contador(n) {
  if (process.platform !== 'win32') return app.setBadgeCount(n);
  janela.setOverlayIcon(n ? bolinha() : null, n ? `${n} não lidas` : '');
}
function bolinha() {
  const t = 16, px = Buffer.alloc(t * t * 4);
  for (let i = 0; i < t * t; i++) {
    const x = (i % t) - 7.5, y = Math.floor(i / t) - 7.5;
    if (x * x + y * y <= 56) px.set([0x45, 0x42, 0xed, 0xff], i * 4); // BGRA, vermelho
  }
  return nativeImage.createFromBitmap(px, { width: t, height: t });
}

// Atalhos e apertar para falar escolhidos no site, com o app em segundo plano. O uiohook só escuta o teclado (como o
// Discord): a tecla continua indo para o programa em uso. Responde se está escutando (senão o site cuida só com foco).
// O site manda no máximo 10 atalhos + 1 tecla de falar: não dá para virar um leitor de tudo que se digita.
let gancho = null, escolhidas = { atalhos: [], ptt: null };
const apertadas = new Set();
function teclas({ atalhos, ptt }) {
  let ui;
  try { ui = require('uiohook-napi'); } catch { return false; }
  // KeyboardEvent.code ("KeyM", "Digit1", "ControlLeft") vira o nome do uiohook ("M", "1", "Ctrl")
  const tecla = (code) => ui.UiohookKey[code.replace(/^(Key|Digit)/, '').replace(/^Control/, 'Ctrl').replace(/^(Ctrl|Alt|Shift|Meta)Left$/, '$1')];
  escolhidas = {
    atalhos: atalhos.slice(0, 10).map((c) => {
      const p = c.split('+');
      return { c, k: tecla(p.pop()), ctrl: p.includes('Ctrl'), alt: p.includes('Alt'), shift: p.includes('Shift'), meta: p.includes('Meta') };
    }).filter((a) => a.k),
    ptt: ptt && tecla(ptt),
  };
  if (gancho) return true;
  if (!escolhidas.atalhos.length && !escolhidas.ptt) return false;
  // Mac: precisa de Acessibilidade; a 1ª vez o sistema pede e só vale depois de reabrir o app
  if (process.platform === 'darwin' && !systemPreferences.isTrustedAccessibilityClient(true)) return false;
  gancho = ui.uIOhook;
  gancho.on('keydown', (e) => {
    if (apertadas.has(e.keycode)) return; // tecla segurada repete o keydown
    apertadas.add(e.keycode);
    if (e.keycode === escolhidas.ptt) janela.webContents.send('falar', true);
    const a = escolhidas.atalhos.find((x) => x.k === e.keycode && x.ctrl === e.ctrlKey && x.alt === e.altKey && x.shift === e.shiftKey && x.meta === e.metaKey);
    if (a) janela.webContents.send('atalho', a.c);
  });
  gancho.on('keyup', (e) => {
    apertadas.delete(e.keycode);
    if (e.keycode === escolhidas.ptt) janela.webContents.send('falar', false);
  });
  gancho.start();
  app.on('will-quit', () => gancho.stop()); // sem isso o processo cai ao fechar
  return true;
}

// Abre o seletor e devolve a fonte escolhida (null = cancelou ou sem permissão).
async function escolher() {
  const fontes = await desktopCapturer.getSources({ types: ['screen', 'window'], fetchWindowIcons: true }).catch(() => []);
  if (process.platform === 'win32') fontes.push(...(await janelasEscondidas(fontes)));
  if (!fontes.length) {
    if (process.platform === 'darwin') {
      dialog.showMessageBox({ message: 'Libere o app em Ajustes > Privacidade e Segurança > Gravação de Tela e Áudio do Sistema, depois reabra o app.' })
        .then(() => shell.openExternal('x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture'));
    }
    return null;
  }
  // Linux/Wayland: o seletor do sistema já escolheu e só vem uma fonte
  // fechar o seletor no X pode rejeitar no meio do caminho: sem responder, o pedido trava e o app não compartilha mais
  const id = fontes.length === 1 ? fontes[0].id : await escolherFonte(fontes).catch(() => null);
  return fontes.find((f) => f.id === id) ?? null;
}

async function escolherFonte(fontes) {
  const visiveis = fontes
    .filter((f) => f.name && f.name !== app.getName())
    // macOS: janelas sem miniatura são internas (Stage Manager, overlays). No Windows é janela minimizada,
    // como um jogo em tela cheia quando se dá alt-tab: entra com o ícone do app no lugar da miniatura.
    .filter((f) => process.platform !== 'darwin' || !f.thumbnail.isEmpty())
    .filter((f) => !['App Icon Window', 'Gesture Blocking Overlay'].includes(f.name))
    .map((f) => ({ id: f.id, nome: f.name, miniatura: (f.thumbnail.isEmpty() ? f.appIcon : f.thumbnail)?.toDataURL() }));
  const seletor = new BrowserWindow({
    parent: janela, modal: true, width: 900, height: 600, autoHideMenuBar: true, backgroundColor: '#16191A',
  });
  await seletor.loadFile(path.join(__dirname, 'seletor.html'));
  const fechou = new Promise((resolve) => seletor.once('closed', () => resolve(null)));
  const id = await Promise.race([seletor.webContents.executeJavaScript(`escolher(${JSON.stringify(visiveis)})`).catch(() => null), fechou]);
  if (!seletor.isDestroyed()) seletor.close();
  return id;
}

// Windows: janelas que o Chromium não lista (jogo em tela cheia não responde a tempo à checagem dele).
// O id "window:<hwnd>:0" é o mesmo formato dele, então a captura funciona igual.
function janelasEscondidas(fontes) {
  return new Promise((resolve) => {
    execFile(nativo, ['--janelas'], { timeout: 3000 }, (erro, saida) => {
      if (erro) return resolve([]);
      const listadas = new Set(fontes.map((f) => f.id.split(':')[1]));
      resolve(saida.split('\n').map((l) => l.split('\t')).filter(([hwnd, pid, nome]) => nome && Number(pid) !== process.pid && !listadas.has(hwnd))
        .map(([hwnd, , nome]) => ({ id: `window:${hwnd}:0`, name: nome.trim(), thumbnail: nativeImage.createEmpty(), appIcon: null })));
    });
  });
}

// Windows: roda o telao-som.exe no processo da janela ("window:<hwnd>:0") e repassa o PCM para a página.
function somDaJanela(pagina, id) {
  captura?.kill();
  captura = null;
  if (!id) return;
  captura = spawn(nativo, [id.split(':')[1]]);
  let resto = Buffer.alloc(0);
  captura.stdout.on('data', (dados) => {
    // o pipe corta em qualquer byte; manda só quadros inteiros (4 bytes: 2 canais de 16 bits)
    const tudo = Buffer.concat([resto, dados]);
    const n = tudo.length - (tudo.length % 4);
    resto = tudo.subarray(n);
    if (n && !pagina.isDestroyed()) pagina.send('som-janela', tudo.subarray(0, n));
  });
  captura.on('error', (e) => console.error('telao-som:', e.message));
  captura.on('exit', (codigo) => codigo && console.error('telao-som saiu com', codigo));
}

async function diagnostico() {
  const texto = await janela.webContents.executeJavaScript(`(async () => {
    const pub = window.__voice?.room.localParticipant.getTrackPublication('screen_share');
    if (!pub?.track) return 'Você não está transmitindo a tela.';
    const c = pub.track.mediaStreamTrack.getSettings();
    const linhas = ['Captura: ' + c.width + 'x' + c.height + ' @ ' + c.frameRate];
    (await pub.track.sender.getStats()).forEach((s) => {
      if (s.type !== 'outbound-rtp' || s.kind !== 'video') return;
      const d = s.qualityLimitationDurations || {};
      linhas.push('Enviando: ' + s.frameWidth + 'x' + s.frameHeight + ' a ' + s.framesPerSecond + ' fps',
        'Limitado por: ' + s.qualityLimitationReason + ' (cpu ' + Math.round(d.cpu || 0) + 's, banda ' + Math.round(d.bandwidth || 0) + 's)',
        'Codificador: ' + s.encoderImplementation + (s.powerEfficientEncoder ? ' (placa)' : ' (processador)'),
        'Codificação: ' + (1000 * s.totalEncodeTime / s.framesEncoded).toFixed(1) + ' ms/quadro',
        'Alvo: ' + Math.round((s.targetBitrate || 0) / 1000) + ' kbps');
    });
    return linhas.join('\\n');
  })()`).catch((e) => 'Erro: ' + e.message);
  let janelas = '';
  if (process.platform === 'win32') {
    // onde o jogo some: na lista do Chromium, na do Windows (telao-som --janelas) ou nas duas
    const chromium = await desktopCapturer.getSources({ types: ['window'] }).catch(() => []);
    const windows = await new Promise((r) => execFile(nativo, ['--janelas'], { timeout: 3000 }, (e, saida) => r(e ? `erro: ${e.message}` : saida.split('\n').map((l) => l.split('\t')[2]).filter(Boolean).join(' | '))));
    janelas = `\nJanelas (Chromium): ${chromium.map((f) => f.name).join(' | ')}\nJanelas (Windows): ${windows}`;
  }
  const relatorio = `Telão ${app.getVersion()} · ${process.platform}\n${texto}${janelas}`;
  const { response } = await dialog.showMessageBox(janela, { message: 'Diagnóstico da transmissão', detail: relatorio, buttons: ['Copiar', 'Fechar'] });
  if (response === 0) clipboard.writeText(relatorio);
}

// Baixa a versão nova em segundo plano e pergunta se reinicia agora; "Depois" instala ao fechar o app.
// Windows/Linux: electron-updater. Mac: troca o próprio Telao.app (o atualizador do Electron no Mac exige assinatura da Apple).
let pronta = false;
function atualizar() {
  const checar = () => !pronta && (process.platform === 'darwin' ? atualizarMac() : autoUpdater.checkForUpdates()).catch((e) => console.error('atualização:', e.message));
  autoUpdater.on('update-downloaded', (i) => perguntar(i.version, () => autoUpdater.quitAndInstall(true, true)));
  checar();
  setInterval(checar, 30 * 60 * 1000);
}

async function perguntar(versao, reiniciar) {
  if (pronta) return;
  pronta = true;
  const { response } = await dialog.showMessageBox(janela, {
    message: `Telão ${versao.replace(/^v/, '')} pronto`,
    detail: 'Reinicie para usar a versão nova (leva poucos segundos). Se preferir, ela entra sozinha quando você fechar o app.',
    buttons: ['Reiniciar agora', 'Depois'],
  });
  if (response === 0) reiniciar();
}

async function atualizarMac() {
  const ultima = await fetch('https://api.github.com/repos/telao-app/desktop/releases/latest').then((r) => r.json());
  const partes = (v) => String(v).replace(/^v/, '').split(/[.-]/).slice(0, 3).map(Number);
  const [a, b] = [partes(ultima.tag_name), partes(app.getVersion())];
  const nova = a.findIndex((n, i) => n !== b[i]);
  if (nova < 0 || !(a[nova] > b[nova])) return;
  const zip = ultima.assets?.find((x) => x.name === 'Telao-mac.zip');
  const atual = path.resolve(process.execPath, '../../..'); // .../Telao.app
  try {
    fs.accessSync(path.dirname(atual), fs.constants.W_OK); // aberto do dmg ou de pasta sem permissão: não dá para trocar
    if (!zip) throw new Error('release sem zip');
  } catch {
    pronta = true;
    const { response } = await dialog.showMessageBox(janela, { message: `Versão nova do Telão: ${ultima.tag_name}`, buttons: ['Baixar', 'Depois'] });
    if (response === 0) shell.openExternal('https://github.com/telao-app/desktop/releases/latest/download/Telao-mac.dmg');
    return;
  }
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'telao-'));
  const arquivo = path.join(dir, 'Telao.zip');
  fs.writeFileSync(arquivo, Buffer.from(await (await fetch(zip.browser_download_url)).arrayBuffer()));
  await new Promise((ok, erro) => execFile('ditto', ['-x', '-k', arquivo, dir], (e) => (e ? erro(e) : ok())));
  // troca depois que este processo sai. Só o Contents: a pasta Telao.app fica a mesma e o atalho do Dock continua
  // apontando para ela. O Contents antigo vai para a pasta temporária (o sistema limpa) e volta se a troca falhar.
  let abrir = '0';
  app.once('will-quit', () => spawn('/bin/sh', ['-c',
    'while kill -0 "$0" 2>/dev/null; do sleep 0.2; done; mv "$1/Contents" "$2.velho" && { mv "$2/Contents" "$1/Contents" || mv "$2.velho" "$1/Contents"; }; [ "$3" = 1 ] && open "$1"',
    String(process.pid), atual, path.join(dir, 'Telao.app'), abrir], { detached: true, stdio: 'ignore' }).unref());
  perguntar(ultima.tag_name, () => { abrir = '1'; app.quit(); });
}

app.on('window-all-closed', () => app.quit());
