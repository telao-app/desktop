const path = require('node:path');
const { execFile, spawn } = require('node:child_process');
const { app, BrowserWindow, clipboard, desktopCapturer, dialog, ipcMain, nativeImage, session, shell, systemPreferences } = require('electron');
const { autoUpdater } = require('electron-updater');

// ponytail: carrega o site; quando o front for empacotado aqui dentro, deixa de depender do servidor entregar o código
const URL_TELAO = process.env.TELAO_URL || 'https://telao.nyxdev.com.br';

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
    width: 1280, height: 800, autoHideMenuBar: true, backgroundColor: '#111214',
    webPreferences: { preload: path.join(__dirname, 'preload.js') },
  });
  janela.loadURL(URL_TELAO);
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
    parent: janela, modal: true, width: 900, height: 600, autoHideMenuBar: true, backgroundColor: '#111214',
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
  const { response } = await dialog.showMessageBox(janela, { message: 'Diagnóstico da transmissão', detail: `Telão ${app.getVersion()} · ${process.platform}\n${texto}`, buttons: ['Copiar', 'Fechar'] });
  if (response === 0) clipboard.writeText(`Telão ${app.getVersion()} · ${process.platform}\n${texto}`);
}

// Windows/Linux: baixa a versão nova das releases do GitHub e instala ao fechar o app.
// Mac: a atualização automática exige app assinado pela Apple; só avisa e abre o download.
async function atualizar() {
  if (process.platform !== 'darwin') return autoUpdater.checkForUpdatesAndNotify().catch((e) => console.error('atualização:', e.message));
  const ultima = await fetch('https://api.github.com/repos/telao-app/telao-app-desktop/releases/latest').then((r) => r.json()).catch(() => null);
  const partes = (v) => String(v).replace(/^v/, '').split(/[.-]/).slice(0, 3).map(Number);
  const [a, b] = [partes(ultima?.tag_name), partes(app.getVersion())];
  const nova = a.findIndex((n, i) => n !== b[i]);
  if (nova < 0 || !(a[nova] > b[nova])) return;
  const { response } = await dialog.showMessageBox({ message: `Versão nova do Telão: ${ultima.tag_name}`, buttons: ['Baixar', 'Depois'] });
  if (response === 0) shell.openExternal('https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-mac.dmg');
}

app.on('window-all-closed', () => app.quit());
