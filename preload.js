// Windows: ao transmitir uma janela, o som vem só do programa dela (não do computador todo).
// O seletor abre antes do getDisplayMedia; se for janela, a página recebe o vídeo sem som do sistema
// e uma faixa de áudio montada com o PCM que o nativo/telao-som.exe captura do processo da janela.
const { contextBridge, ipcRenderer } = require('electron');

// Atalhos globais da chamada (o site manda as combinações e recebe quando apertam)
contextBridge.exposeInMainWorld('telaoDesktop', {
  atalhos: (lista, aoApertar) => {
    ipcRenderer.removeAllListeners('atalho');
    ipcRenderer.on('atalho', (_e, c) => aoApertar(c));
    ipcRenderer.send('atalhos', lista);
  },
});

if (process.platform === 'win32') {
  const desktop = {
    escolher: () => ipcRenderer.invoke('escolher-fonte'),
    somDaJanela: (id, aoChegar) => {
      ipcRenderer.removeAllListeners('som-janela');
      // passa ArrayBuffer novo: atravessa o contextBridge e já vem alinhado para Int16Array
      if (aoChegar) ipcRenderer.on('som-janela', (_e, pcm) => aoChegar(pcm.buffer.slice(pcm.byteOffset, pcm.byteOffset + pcm.byteLength)));
      ipcRenderer.send('som-janela', id);
    },
  };

  contextBridge.executeInMainWorld({
    args: [desktop],
    func: (desktop) => {
      // toca o PCM (int16 estéreo 48 kHz) que chega do processo; se acumular atraso, descarta o excesso
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
      const original = navigator.mediaDevices.getDisplayMedia.bind(navigator.mediaDevices);
      navigator.mediaDevices.getDisplayMedia = async (pedido = {}) => {
        const fonte = await desktop.escolher();
        if (!fonte) throw new DOMException('Compartilhamento cancelado', 'AbortError');
        if (!fonte.janela || !pedido.audio) return original(pedido);

        const stream = await original({ ...pedido, audio: false });
        const ac = new AudioContext({ sampleRate: 48000 });
        await ac.audioWorklet.addModule(URL.createObjectURL(new Blob([worklet], { type: 'text/javascript' })));
        const no = new AudioWorkletNode(ac, 'telao-som', { outputChannelCount: [2] });
        const destino = ac.createMediaStreamDestination();
        no.connect(destino);
        desktop.somDaJanela(fonte.id, (pcm) => no.port.postMessage(pcm, [pcm]));
        const som = destino.stream.getAudioTracks()[0];
        stream.addTrack(som);

        // parar a transmissão (stop() da página ou a janela fechando) encerra a captura do processo
        let parado = false;
        const parar = () => { if (!parado) { parado = true; desktop.somDaJanela(null); ac.close(); } };
        for (const t of stream.getTracks()) {
          const stop = t.stop.bind(t);
          t.stop = () => { stop(); parar(); };
          t.addEventListener('ended', parar);
        }
        return stream;
      };
    },
  });
}
