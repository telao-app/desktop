# telao-app-desktop

App desktop do Telão: uma janela que carrega `https://telao.nyxdev.com.br` e acrescenta o que o navegador não dá.

- **Mac e Windows: Tauri** ([src-tauri](src-tauri)), com o webview do sistema (WebKit no Mac, WebView2 no Windows).
- **Linux: Electron** ([main.js](main.js)): no Linux o webview do sistema (WebKitGTK) não tem WebRTC confiável para a voz.

O que o app acrescenta:

- som junto da transmissão de tela, sem a voz dos colegas que o próprio app toca:
  - Mac (14.2 ou mais novo): o som do computador, por um tap do CoreAudio que exclui os processos do Telão;
  - Windows: tela inteira com o som do sistema (opção do seletor); janela com o som só do programa dela
    ([nativo/telao-som.cpp](nativo/telao-som.cpp));
  - Linux: ainda não;
- atalhos e apertar para falar com o app em segundo plano (só escuta o teclado, não rouba a tecla);
- não lidas no ícone (número no Dock, bolinha na barra de tarefas do Windows);
- Ctrl+Shift+D (⌘⇧D no Mac): diagnóstico da transmissão;
- atualização sozinha: baixa em segundo plano e pergunta se reinicia.

O seletor de tela é o do sistema: no Mac o do macOS, no Windows o do WebView2 (com miniaturas). O app Electron
tinha um seletor próprio que também mostrava jogo em tela cheia escondido no Windows; no Tauri esse jogo entra
transmitindo a tela inteira.

## Baixar

- Mac (Apple Silicon): https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-mac.dmg
- Windows: https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-windows.exe
- Linux: https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-linux.AppImage

O app não é assinado pela Apple nem pela Microsoft. No Mac, depois de arrastar para Aplicativos, rode uma vez
`xattr -cr /Applications/Telao.app`; no Windows, em "O Windows protegeu o computador", clique em
*Mais informações > Executar assim mesmo*.

## Desenvolver

Mac e Windows (precisa de Rust):

```bash
npm install && npm run start:tauri                          # abre o site de produção
TELAO_URL=http://localhost:5173 npm run start:tauri         # abre o web local
```

Linux (Electron):

```bash
npm install && npm start
TELAO_URL=http://localhost:5173 npm start
```

O site só alcança os comandos do app a partir das origens em
[src-tauri/capabilities/site.json](src-tauri/capabilities/site.json) (produção e `localhost:5173`); plugins do
Tauri ficam fechados para ele. A ponte com o site está em [src-tauri/src/ponte.js](src-tauri/src/ponte.js)
(`window.telaoDesktop`, o mesmo contrato do preload do Electron).

No Mac, os atalhos em segundo plano pedem Acessibilidade e o som da transmissão pede "Gravação de Áudio do
Sistema". Rodando pelo terminal, a permissão vai para o terminal; para testar, abra o app empacotado
(`npm run dist:tauri`, depois `src-tauri/target/release/bundle/macos/Telao.app`).

## Nova versão

```bash
npm version patch && git push --follow-tags
```

O GitHub Actions ([release.yml](.github/workflows/release.yml)) gera `.dmg` e `.zip` (Mac), `.exe` (Windows) e
`.AppImage` (Linux) e publica a release. A versão vem do `package.json` para os dois. O nome interno é `Telao`
(sem acento); o nome que aparece no Mac é "Telão".

### Quem tinha o app Electron (0.1.x) no Mac ou no Windows

Atualiza sozinho para o Tauri: no Mac o Electron baixa o `Telao-mac.zip` e troca o `Contents` (mesmo id e mesmo
certificado, então a permissão de gravar a tela continua); no Windows ele lê o `latest.yml` que o release gera
para o instalador do Tauri, que remove o Electron antigo e abre o app novo
([migrar-electron.nsh](src-tauri/windows/migrar-electron.nsh)). Os dados do site ficam no navegador de cada
motor, então na primeira vez no app novo é preciso entrar de novo na conta.
