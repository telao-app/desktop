const path = require('node:path');
const { app, BrowserWindow, desktopCapturer, dialog, session, shell, systemPreferences } = require('electron');

// ponytail: carrega o site; quando o front for empacotado aqui dentro, deixa de depender do servidor entregar o código
const URL_TELAO = process.env.TELAO_URL || 'https://telao.nyxdev.com.br';

app.whenReady().then(async () => {
  if (process.platform === 'darwin') {
    await systemPreferences.askForMediaAccess('microphone');
    await systemPreferences.askForMediaAccess('camera');
  }
  session.defaultSession.setDisplayMediaRequestHandler(async (req, callback) => {
    const fontes = await desktopCapturer.getSources({ types: ['screen', 'window'], fetchWindowIcons: true }).catch(() => []);
    if (!fontes.length) {
      callback(null);
      if (process.platform === 'darwin') {
        await dialog.showMessageBox({ message: 'Libere o app em Ajustes > Privacidade e Segurança > Gravação de Tela e Áudio do Sistema, depois reabra o app.' });
        shell.openExternal('x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture');
      }
      return;
    }
    // Linux/Wayland: o seletor do sistema já escolheu e só vem uma fonte
    // fechar o seletor no X pode rejeitar no meio do caminho: sem responder, o pedido trava e o app não compartilha mais
    const id = fontes.length === 1 ? fontes[0].id : await escolherFonte(fontes).catch(() => null);
    const escolha = fontes.find((f) => f.id === id);
    if (!escolha) return callback(null);
    // som do sistema só se a página pediu (entregar sem pedir, ou não entregar o pedido, quebra a captura).
    // No Mac só funciona com o app aberto por conta própria: aberto por terminal, o macOS dá a permissão ao terminal.
    // ponytail: Linux sem som do sistema; dá para capturar o monitor do PipeWire como microfone
    const som = req.audioRequested && process.platform !== 'linux';
    callback({ video: escolha, audio: som ? 'loopback' : undefined });
  });

  janela = new BrowserWindow({ width: 1280, height: 800, autoHideMenuBar: true, backgroundColor: '#111214' });
  janela.loadURL(URL_TELAO);
});

let janela;

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

app.on('window-all-closed', () => app.quit());
