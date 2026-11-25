// Só o site do Telão fala com o processo principal (achado do Strix): qualquer outra página que acabe carregada na
// janela (link aberto dentro do app, iframe, redirecionamento) não chama os comandos nativos (teclas globais, seletor
// de tela, som da janela). Compara a ORIGEM do quadro que mandou a mensagem com a de URL_TELAO.
const origemDe = (url) => {
  try {
    const o = new URL(url).origin;
    return o && o !== 'null' ? o : null; // file:, data:, about: não têm origem comparável
  } catch {
    return null;
  }
};

/** O quadro que mandou a mensagem é do Telão? Sem quadro (destruído, navegou) também é recusado. */
function remetentePermitido(evento, base) {
  const url = evento?.senderFrame?.url;
  const de = url ? origemDe(url) : null;
  return de !== null && de === origemDe(base);
}

/**
 * Envolve um ipcMain.handle/on: de origem diferente, recusa e registra no console. `handle` rejeita a chamada (o site
 * recebe o erro); `on` só ignora.
 */
function protegido(base, canal, fn, { rejeitar = true, log = console.warn } = {}) {
  return (evento, ...args) => {
    if (!remetentePermitido(evento, base)) {
      log(`IPC recusado: "${canal}" de ${evento?.senderFrame?.url || 'origem desconhecida'} (só ${origemDe(base)})`);
      if (rejeitar) throw new Error('origem não permitida');
      return undefined;
    }
    return fn(evento, ...args);
  };
}

module.exports = { origemDe, remetentePermitido, protegido };
