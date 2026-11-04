# telao-app-desktop

App desktop do Telão (Mac, Windows e Linux): uma janela Electron que carrega
`https://telao.nyxdev.com.br` e acrescenta o que o navegador não dá:

- seletor de tela/janela com miniaturas;
- som do sistema junto da transmissão (Windows e Mac; Linux ainda não), sem a voz dos colegas que o próprio app toca.

## Baixar

- Mac (Apple Silicon): https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-mac.dmg
- Windows: https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-windows.exe
- Linux: https://github.com/telao-app/telao-app-desktop/releases/latest/download/Telao-linux.AppImage

O app não é assinado. No Mac, depois de arrastar para Aplicativos, rode uma vez
`xattr -cr /Applications/Telao.app`; no Windows, em "O Windows protegeu o computador", clique em
*Mais informações > Executar assim mesmo*.

## Desenvolver

```bash
npm install && npm start                      # abre o site de produção
TELAO_URL=http://localhost:5173 npm start     # abre o web local
```

No Mac, rodando pelo terminal a permissão de câmera/gravação vai para o terminal e o som do
sistema chega mudo; para testar som e câmera, abra como app próprio:
`open -n node_modules/electron/dist/Electron.app --args "$PWD"`.

## Nova versão

```bash
npm version patch && git push --follow-tags
```

O GitHub Actions ([release.yml](.github/workflows/release.yml)) gera `.dmg`, `.exe` e `.AppImage`
e publica a release. O nome interno é `Telao` (sem acento): com "ã" o Electron empacotado quebra ao abrir.
