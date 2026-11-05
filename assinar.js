// Mac: assina com o certificado próprio do Telão (no CI, MAC_ASSINATURA=Telao). Assinatura estável entre versões =
// o macOS lembra a permissão de gravar a tela depois de atualizar. Sem ela (build local), fica a ad-hoc do electron-builder.
const { execFileSync } = require('node:child_process');

exports.default = ({ electronPlatformName, appOutDir, packager }) => {
  if (electronPlatformName !== 'darwin' || !process.env.MAC_ASSINATURA) return;
  execFileSync('codesign', ['--force', '--deep', '--sign', process.env.MAC_ASSINATURA, `${appOutDir}/${packager.appInfo.productFilename}.app`], { stdio: 'inherit' });
};
