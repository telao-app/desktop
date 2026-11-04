// Captura o som de um único programa no Windows (loopback por processo, Windows 10 2004+) e escreve no stdout
// como PCM 16 bits, 48 kHz, estéreo. Uso: telao-som.exe <hwnd da janela escolhida>
// telao-som.exe --janelas lista as janelas ("hwnd<TAB>pid<TAB>título" em UTF-8): o seletor do Chromium esconde
// jogos em tela cheia (não respondem a tempo à checagem dele) e o app junta esta lista à dele.
// Baseado no ApplicationLoopback do Windows-classic-samples da Microsoft.
#include <windows.h>
#include <audioclient.h>
#include <audioclientactivationparams.h>
#include <mmdeviceapi.h>
#include <wrl/implements.h>
#include <cstdio>
#include <cstdlib>
#include <cwchar>
#include <dwmapi.h>
#include <fcntl.h>
#include <io.h>
#include <vector>

using Microsoft::WRL::ComPtr;

class Ativacao : public Microsoft::WRL::RuntimeClass<Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
                                                     Microsoft::WRL::FtmBase, IActivateAudioInterfaceCompletionHandler> {
 public:
  HANDLE pronto = CreateEvent(nullptr, FALSE, FALSE, nullptr);
  HRESULT resultado = E_FAIL;
  ComPtr<IAudioClient> cliente;

  STDMETHOD(ActivateCompleted)(IActivateAudioInterfaceAsyncOperation* op) override {
    HRESULT hr;
    ComPtr<IUnknown> interface_;
    resultado = op->GetActivateResult(&hr, &interface_);
    if (SUCCEEDED(resultado)) resultado = hr;
    if (SUCCEEDED(resultado)) resultado = interface_.As(&cliente);
    SetEvent(pronto);
    return S_OK;
  }
};

static BOOL CALLBACK listar(HWND janela, LPARAM) {
  if (!IsWindowVisible(janela) || GetWindow(janela, GW_OWNER) || (GetWindowLongW(janela, GWL_EXSTYLE) & WS_EX_TOOLWINDOW))
    return TRUE;
  BOOL oculta = FALSE;  // janelas "cloaked": apps da loja suspensos, outras áreas de trabalho
  DwmGetWindowAttribute(janela, DWMWA_CLOAKED, &oculta, sizeof oculta);
  wchar_t titulo[512];
  int n = GetWindowTextW(janela, titulo, 512);
  if (oculta || !n) return TRUE;
  for (int i = 0; i < n; i++)
    if (titulo[i] == L'\t' || titulo[i] == L'\n' || titulo[i] == L'\r') titulo[i] = L' ';
  char utf8[2048];
  int m = WideCharToMultiByte(CP_UTF8, 0, titulo, n, utf8, sizeof utf8, nullptr, nullptr);
  DWORD pid = 0;
  GetWindowThreadProcessId(janela, &pid);
  printf("%lld\t%lu\t%.*s\n", static_cast<long long>(reinterpret_cast<INT_PTR>(janela)), pid, m, utf8);
  return TRUE;
}

int wmain(int argc, wchar_t** argv) {
  if (argc < 2) return 2;
  if (wcscmp(argv[1], L"--janelas") == 0) return EnumWindows(listar, 0) ? 0 : 1;
  DWORD pid = 0;
  GetWindowThreadProcessId(reinterpret_cast<HWND>(static_cast<INT_PTR>(_wtoi64(argv[1]))), &pid);
  if (!pid) return 3;
  if (FAILED(CoInitializeEx(nullptr, COINIT_MULTITHREADED))) return 4;

  // a árvore inteira do processo: jogo com launcher às vezes toca o som num processo filho
  AUDIOCLIENT_ACTIVATION_PARAMS params = {};
  params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
  params.ProcessLoopbackParams.TargetProcessId = pid;
  params.ProcessLoopbackParams.ProcessLoopbackMode = PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;
  PROPVARIANT pv = {};
  pv.vt = VT_BLOB;
  pv.blob.cbSize = sizeof(params);
  pv.blob.pBlobData = reinterpret_cast<BYTE*>(&params);

  auto ativacao = Microsoft::WRL::Make<Ativacao>();
  ComPtr<IActivateAudioInterfaceAsyncOperation> op;
  if (FAILED(ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, __uuidof(IAudioClient), &pv,
                                         ativacao.Get(), &op)))
    return 5;
  WaitForSingleObject(ativacao->pronto, INFINITE);
  if (FAILED(ativacao->resultado)) return 6;

  // loopback por processo não informa o formato de mixagem: pede o nosso e o Windows converte
  WAVEFORMATEX formato = {};
  formato.wFormatTag = WAVE_FORMAT_PCM;
  formato.nChannels = 2;
  formato.nSamplesPerSec = 48000;
  formato.wBitsPerSample = 16;
  formato.nBlockAlign = 4;
  formato.nAvgBytesPerSec = 48000 * 4;
  auto& cliente = ativacao->cliente;
  if (FAILED(cliente->Initialize(AUDCLNT_SHAREMODE_SHARED,
                                 AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK |
                                     AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                                 200000, 0, &formato, nullptr)))
    return 7;
  HANDLE chegou = CreateEvent(nullptr, FALSE, FALSE, nullptr);
  ComPtr<IAudioCaptureClient> captura;
  if (FAILED(cliente->SetEventHandle(chegou)) || FAILED(cliente->GetService(IID_PPV_ARGS(&captura)))) return 8;

  _setmode(_fileno(stdout), _O_BINARY);
  std::vector<BYTE> silencio;
  if (FAILED(cliente->Start())) return 9;
  for (;;) {
    WaitForSingleObject(chegou, 100);
    UINT32 pacote = 0;
    while (SUCCEEDED(captura->GetNextPacketSize(&pacote)) && pacote) {
      BYTE* dados;
      UINT32 quadros;
      DWORD flags;
      if (FAILED(captura->GetBuffer(&dados, &quadros, &flags, nullptr, nullptr))) return 10;
      if (flags & AUDCLNT_BUFFERFLAGS_SILENT) {
        silencio.assign(quadros * 4, 0);
        dados = silencio.data();
      }
      fwrite(dados, 4, quadros, stdout);
      captura->ReleaseBuffer(quadros);
    }
    if (fflush(stdout) != 0) return 0;  // o app fechou o pipe: parou de transmitir
  }
}
