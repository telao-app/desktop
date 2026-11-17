// Captura própria de tela e de janela no Windows (Windows Graphics Capture, a base do Discord e do OBS).
// O seletor do WebView2 não lista jogo em tela cheia exclusiva (fica minimizado enquanto a pessoa está fora dele) e recusa
// pedir uma janela pelo id. Aqui o app lista tudo (janelas, jogos, telas) para o seletor próprio do ponte.js, captura a
// escolhida e entrega cada quadro à página por memória compartilhada do WebView2 (PostSharedBufferToScript); lá ele vira
// a faixa de vídeo (MediaStreamTrackGenerator). Mac: segue o seletor do sistema.
//
// Memória de cada quadro: CABECALHO bytes (o primeiro i32 é 1 enquanto a página não copiou o quadro, 0 quando está livre)
// e depois a imagem sem sobra no fim das linhas: NV12 já no tamanho de envio (reduzida e convertida pela placa, como o
// OBS faz), ou BGRA do tamanho da fonte se a placa não tiver o processador de vídeo do Direct3D. Três buffers giram; se a página atrasa, o quadro é descartado.
use serde::Serialize;
use std::sync::Mutex;
use tauri::ipc::{Channel, Response};
use tauri::{State, Webview};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fonte {
  /// "janela:<hwnd>" ou "tela:<hmonitor>"
  id: String,
  /// "janela" ou "tela"
  tipo: &'static str,
  /// título da janela, ou "Tela 1"
  nome: String,
  /// programa da janela ("chrome"), ou a resolução da tela
  programa: String,
  /// nome do jogo (pela pasta da loja), se for um
  jogo: Option<String>,
  /// minimizada: sem miniatura, e a imagem só chega quando ela volta (jogo em tela cheia: quando a pessoa volta pro jogo)
  minimizada: bool,
  /// tela principal do Windows
  principal: bool,
  largura: u32,
  altura: u32,
}

#[derive(Default)]
pub struct Estado(Mutex<Option<Sessao>>);

#[cfg(windows)]
use win::Sessao;
#[cfg(not(windows))]
pub struct Sessao;

#[tauri::command(async)]
pub fn fontes() -> Vec<Fonte> {
  #[cfg(windows)]
  return win::fontes();
  #[cfg(not(windows))]
  Vec::new()
}

/// Miniatura da fonte: 8 bytes (largura, altura em u32) e a imagem RGBA; vazio quando não deu (janela minimizada).
#[tauri::command(async)]
pub fn miniatura(id: String) -> Response {
  #[cfg(windows)]
  return Response::new(win::miniatura(&id).unwrap_or_default());
  #[cfg(not(windows))]
  {
    let _ = id;
    Response::new(Vec::<u8>::new())
  }
}

/// Começa a capturar e devolve o número da sessão (vai em cada quadro). `eventos` recebe "fim" quando a janela fecha.
/// `maximo`: largura e altura máximas do que sai (0 = o tamanho da fonte).
#[tauri::command]
pub async fn captura(
  webview: Webview,
  estado: State<'_, Estado>,
  id: String,
  fps: u32,
  maximo: Option<(u32, u32)>,
  eventos: Channel<String>,
) -> Result<u32, String> {
  #[cfg(windows)]
  {
    let mut atual = estado.0.lock().unwrap();
    *atual = None; // uma transmissão de tela por vez
    let sessao = win::Sessao::iniciar(webview, &id, fps.clamp(1, 240), maximo.unwrap_or((0, 0)), eventos)?;
    let n = sessao.numero();
    *atual = Some(sessao);
    Ok(n)
  }
  #[cfg(not(windows))]
  {
    let _ = (webview, estado, id, fps, maximo, eventos);
    Err("só no Windows".into())
  }
}

#[tauri::command]
pub async fn captura_parar(estado: State<'_, Estado>, sessao: u32) -> Result<(), String> {
  let mut atual = estado.0.lock().unwrap();
  #[cfg(windows)]
  if atual.as_ref().is_some_and(|s| s.numero() == sessao) {
    *atual = None;
  }
  #[cfg(not(windows))]
  let _ = (&mut atual, sessao);
  Ok(())
}

/// Para o diagnóstico (Ctrl+Shift+D)
#[tauri::command]
pub async fn captura_estado(estado: State<'_, Estado>) -> Result<String, String> {
  #[cfg(windows)]
  return Ok(estado.0.lock().unwrap().as_ref().map(|s| s.relatorio()).unwrap_or_else(|| "parada".into()));
  #[cfg(not(windows))]
  {
    let _ = estado;
    Ok(String::new())
  }
}

#[cfg(windows)]
mod win {
  use super::Fonte;
  use crate::jogo;
  use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
  use std::sync::{mpsc, Arc, Mutex};
  use std::time::{Duration, Instant};
  use tauri::ipc::Channel;
  use tauri::Webview;
  use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment12, ICoreWebView2SharedBuffer, ICoreWebView2_17, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
  };
  use windows::core::{Interface, BOOL, HSTRING};
  use windows::Win32::Foundation::{HWND, LPARAM, RECT};
  use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
    D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
  };
  use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709, DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_RATIONAL,
  };
  use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
  use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
  use std::mem::ManuallyDrop;
  use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WINDOWPLACEMENT, WS_EX_TOOLWINDOW,
  };
  use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
  use windows_capture::frame::Frame;
  use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
  use windows_capture::monitor::Monitor;
  use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings, SecondaryWindowSettings,
    Settings,
  };
  use windows_capture::window::Window;

  type Erro = Box<dyn std::error::Error + Send + Sync>;

  const CABECALHO: usize = 64;
  const BUFFERS: usize = 3;
  const MINIATURA: u32 = 384; // largura máxima
  /// a página não devolveu o buffer nesse tempo (recarregou no meio): volta a usar
  const PRESO: Duration = Duration::from_secs(1);

  // ---------- lista ----------

  pub fn fontes() -> Vec<Fonte> {
    let eu = std::process::id();
    let mut janelas: Vec<HWND> = Vec::new();
    unsafe extern "system" fn juntar(h: HWND, l: LPARAM) -> BOOL {
      unsafe { (*(l.0 as *mut Vec<HWND>)).push(h) };
      true.into()
    }
    unsafe {
      let _ = EnumWindows(Some(juntar), LPARAM(&mut janelas as *mut _ as isize));
    }
    let mut lista: Vec<Fonte> = janelas.into_iter().filter_map(|h| janela(h, eu)).collect();
    lista.sort_by_key(|f| f.jogo.is_none()); // jogos primeiro; o resto na ordem do Windows (a de cima primeiro)
    let principal = Monitor::primary().ok().map(|m| m.as_raw_hmonitor() as usize);
    for (i, m) in Monitor::enumerate().unwrap_or_default().into_iter().enumerate() {
      let (largura, altura) = (m.width().unwrap_or(0), m.height().unwrap_or(0));
      lista.push(Fonte {
        id: format!("tela:{}", m.as_raw_hmonitor() as usize),
        tipo: "tela",
        nome: format!("Tela {}", i + 1),
        programa: format!("{largura}×{altura}"),
        jogo: None,
        minimizada: false,
        principal: principal == Some(m.as_raw_hmonitor() as usize),
        largura,
        altura,
      });
    }
    lista
  }

  // As janelas que a barra de tarefas mostra (sem dona, sem ser de ferramenta, não escondidas pelo Windows), menos as do
  // próprio Telão. Minimizadas entram: é o jeito de aparecer o jogo em tela cheia com a pessoa fora dele.
  fn janela(h: HWND, eu: u32) -> Option<Fonte> {
    unsafe {
      if !IsWindowVisible(h).as_bool() || GetWindow(h, GW_OWNER).is_ok_and(|d| !d.is_invalid()) {
        return None;
      }
      if GetWindowLongPtrW(h, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0 {
        return None;
      }
      let mut escondida = 0u32;
      let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut escondida as *mut u32 as *mut _, 4);
      if escondida != 0 {
        return None;
      }
      let mut t = [0u16; 512];
      let n = GetWindowTextW(h, &mut t) as usize;
      let nome = String::from_utf16_lossy(&t[..n]).trim().to_string();
      let mut c = [0u16; 64];
      let nc = GetClassNameW(h, &mut c) as usize;
      if nome.is_empty() || String::from_utf16_lossy(&c[..nc]) == "Progman" {
        return None;
      }
      let mut pid = 0u32;
      GetWindowThreadProcessId(h, Some(&mut pid));
      if pid == eu {
        return None;
      }
      let caminho = jogo::caminho(pid);
      let minimizada = IsIconic(h).as_bool();
      let r = if minimizada {
        // o tamanho de quando ela volta (jogo em tela cheia: o da tela)
        let mut p = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        let _ = GetWindowPlacement(h, &mut p);
        p.rcNormalPosition
      } else {
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        r
      };
      let (largura, altura) = ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32);
      if !minimizada && (largura < 40 || altura < 40) {
        return None;
      }
      let programa = caminho
        .as_deref()
        .and_then(|c| std::path::Path::new(c).file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
      Some(Fonte {
        id: format!("janela:{}", h.0 as usize),
        tipo: "janela",
        jogo: caminho.as_deref().and_then(jogo::nome),
        nome,
        programa,
        minimizada,
        principal: false,
        largura,
        altura,
      })
    }
  }

  // ---------- alvo ----------

  #[derive(Clone, Copy)]
  enum Alvo {
    Janela(usize),
    Tela(usize),
  }

  impl Alvo {
    fn de(id: &str) -> Result<Self, String> {
      let (tipo, n) = id.split_once(':').ok_or("fonte inválida")?;
      let n: usize = n.parse().map_err(|_| "fonte inválida")?;
      match tipo {
        "janela" if unsafe { IsWindow(Some(HWND(n as *mut _))).as_bool() } => Ok(Alvo::Janela(n)),
        "janela" => Err("a janela fechou".into()),
        "tela" => Ok(Alvo::Tela(n)),
        _ => Err("fonte inválida".into()),
      }
    }

    fn minimizada(self) -> bool {
      matches!(self, Alvo::Janela(h) if unsafe { IsIconic(HWND(h as *mut _)).as_bool() })
    }

    fn iniciar<H>(self, cursor: bool, fps: Option<u32>, flags: H::Flags) -> Result<CaptureControl<H, Erro>, String>
    where
      H: GraphicsCaptureApiHandler<Error = Erro> + Send + 'static,
      H::Flags: Send,
    {
      let cursor = if cursor { CursorCaptureSettings::WithCursor } else { CursorCaptureSettings::WithoutCursor };
      // sem a borda amarela do Windows (Windows 11); limite de quadros quando o Windows deixa
      let borda = if GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false) {
        DrawBorderSettings::WithoutBorder
      } else {
        DrawBorderSettings::Default
      };
      let intervalo = match fps {
        Some(f) if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) => {
          MinimumUpdateIntervalSettings::Custom(Duration::from_secs_f64(1.0 / f as f64))
        }
        _ => MinimumUpdateIntervalSettings::Default,
      };
      let (sec, sujo, cor) = (SecondaryWindowSettings::Default, DirtyRegionSettings::Default, ColorFormat::Bgra8);
      match self {
        Alvo::Janela(h) => {
          H::start_free_threaded(Settings::new(Window::from_raw_hwnd(h as *mut _), cursor, borda, sec, intervalo, sujo, cor, flags))
        }
        Alvo::Tela(m) => {
          H::start_free_threaded(Settings::new(Monitor::from_raw_hmonitor(m as *mut _), cursor, borda, sec, intervalo, sujo, cor, flags))
        }
      }
      .map_err(|e| format!("captura: {e:?}"))
    }
  }

  // ---------- miniatura ----------

  type Foto = Arc<Mutex<Option<Vec<u8>>>>;
  struct Miniatura(Foto);

  impl GraphicsCaptureApiHandler for Miniatura {
    type Flags = Foto;
    type Error = Erro;
    fn new(ctx: Context<Foto>) -> Result<Self, Erro> {
      Ok(Self(ctx.flags))
    }
    fn on_frame_arrived(&mut self, quadro: &mut Frame, ctl: InternalCaptureControl) -> Result<(), Erro> {
      let (w, h) = (quadro.width(), quadro.height());
      let mut buf = quadro.buffer()?;
      let pitch = buf.row_pitch() as usize;
      let fonte = buf.as_raw_buffer();
      let mw = w.min(MINIATURA).max(1);
      let mh = ((h as u64 * mw as u64) / w.max(1) as u64).max(1) as u32;
      let mut saida = Vec::with_capacity(8 + (mw * mh * 4) as usize);
      saida.extend_from_slice(&mw.to_le_bytes());
      saida.extend_from_slice(&mh.to_le_bytes());
      // média de cada bloco (fica nítido, sem serrilhado), BGRA -> RGBA
      for y in 0..mh {
        let (y0, y1) = ((y * h / mh) as usize, (((y + 1) * h / mh).max(y * h / mh + 1)) as usize);
        for x in 0..mw {
          let (x0, x1) = ((x * w / mw) as usize, (((x + 1) * w / mw).max(x * w / mw + 1)) as usize);
          let mut s = [0u32; 3];
          let passo = ((y1 - y0) * (x1 - x0)).max(1) as u32;
          for yy in y0..y1 {
            let linha = &fonte[yy * pitch..];
            for xx in x0..x1 {
              let p = &linha[xx * 4..xx * 4 + 4];
              s[0] += p[2] as u32;
              s[1] += p[1] as u32;
              s[2] += p[0] as u32;
            }
          }
          saida.extend_from_slice(&[(s[0] / passo) as u8, (s[1] / passo) as u8, (s[2] / passo) as u8, 255]);
        }
      }
      *self.0.lock().unwrap() = Some(saida);
      ctl.stop();
      Ok(())
    }
  }

  pub fn miniatura(id: &str) -> Option<Vec<u8>> {
    let alvo = Alvo::de(id).ok()?;
    if alvo.minimizada() {
      return None;
    }
    let foto: Foto = Arc::default();
    let controle = alvo.iniciar::<Miniatura>(false, None, foto.clone()).ok()?;
    let fim = Instant::now() + Duration::from_millis(1500);
    while foto.lock().unwrap().is_none() && !controle.is_finished() && Instant::now() < fim {
      std::thread::sleep(Duration::from_millis(10));
    }
    let _ = controle.stop();
    let saida = foto.lock().unwrap().take();
    saida
  }

  // ---------- transmissão ----------

  /// COM do WebView2 só se usa na thread da janela; isto só atravessa threads, nunca é usado fora dela
  struct NaJanela<T>(T);
  unsafe impl<T> Send for NaJanela<T> {}
  unsafe impl<T> Sync for NaJanela<T> {}

  struct Buf {
    com: Option<NaJanela<ICoreWebView2SharedBuffer>>,
    ptr: usize,
    enviado: Option<Instant>,
  }

  impl Buf {
    fn livre(&self) -> &AtomicI32 {
      unsafe { &*(self.ptr as *const AtomicI32) }
    }
  }

  struct Buffers {
    tam: (u32, u32, bool),
    geracao: u32,
    lista: Vec<Buf>,
  }

  struct Compartilhado {
    webview: Webview,
    sessao: u32,
    intervalo: Duration,
    maximo: (u32, u32),
    parar: AtomicBool,
    bufs: Mutex<Buffers>,
    eventos: Channel<String>,
    // números do diagnóstico
    quadros: AtomicU32,
    descartados: AtomicU32,
    copia_us: AtomicU64,
    tamanho: AtomicU64,
    fonte: AtomicU64,
    nv12: AtomicBool,
    inicio: Instant,
  }

  impl Compartilhado {
    // o Release dos buffers velhos vai para a thread da janela
    fn soltar(&self, velhos: Vec<Buf>) {
      let coms: Vec<_> = velhos.into_iter().filter_map(|mut b| b.com.take()).collect();
      if !coms.is_empty() {
        let _ = self.webview.with_webview(move |_| drop(coms));
      }
    }
  }

  impl Drop for Compartilhado {
    fn drop(&mut self) {
      let velhos = std::mem::take(&mut self.bufs.get_mut().unwrap().lista);
      self.soltar(velhos);
    }
  }

  // cria os buffers na thread da janela (o ambiente do WebView2 é de lá) e espera
  fn criar(webview: &Webview, bytes: usize) -> Result<Vec<Buf>, String> {
    let (tx, rx) = mpsc::channel();
    webview
      .with_webview(move |w| {
        let r = (|| -> windows::core::Result<Vec<(NaJanela<ICoreWebView2SharedBuffer>, usize)>> {
          let env: ICoreWebView2Environment12 = w.environment().cast()?;
          (0..BUFFERS)
            .map(|_| unsafe {
              let b = env.CreateSharedBuffer(bytes as u64)?;
              let mut p = std::ptr::null_mut();
              b.Buffer(&mut p)?;
              Ok((NaJanela(b), p as usize))
            })
            .collect()
        })();
        let _ = tx.send(r.map_err(|e| format!("memória compartilhada: {e}")));
      })
      .map_err(|e| e.to_string())?;
    let feitos = rx.recv_timeout(Duration::from_secs(3)).map_err(|_| "memória compartilhada: sem resposta".to_string())??;
    Ok(feitos.into_iter().map(|(com, ptr)| Buf { com: Some(com), ptr, enviado: None }).collect())
  }

  fn textura(dev: &ID3D11Device, (w, h): (u32, u32), formato: DXGI_FORMAT, leitura: bool) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
      Width: w,
      Height: h,
      MipLevels: 1,
      ArraySize: 1,
      Format: formato,
      SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
      Usage: if leitura { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
      BindFlags: if leitura { 0 } else { D3D11_BIND_RENDER_TARGET.0 as u32 },
      CPUAccessFlags: if leitura { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
      MiscFlags: 0,
    };
    let mut t = None;
    unsafe { dev.CreateTexture2D(&desc, None, Some(&mut t))? };
    t.ok_or_else(windows::core::Error::empty)
  }

  // textura -> memória compartilhada, linha a linha (a textura pode ter sobra no fim de cada linha). NV12: a parte das
  // cores vem logo depois das `h` linhas de luz, com a mesma largura de linha, e tem h/2 linhas.
  fn ler(ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D, linha: usize, linhas: usize, destino: &mut [u8]) -> windows::core::Result<()> {
    unsafe {
      let mut m = D3D11_MAPPED_SUBRESOURCE::default();
      ctx.Map(tex, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
      let pitch = m.RowPitch as usize;
      let fonte = std::slice::from_raw_parts(m.pData as *const u8, pitch * (linhas - 1) + linha);
      for y in 0..linhas {
        destino[y * linha..(y + 1) * linha].copy_from_slice(&fonte[y * pitch..y * pitch + linha]);
      }
      ctx.Unmap(tex, 0);
    }
    Ok(())
  }

  // Processador de vídeo do Direct3D: reduz e passa de BGRA para NV12 numa passada só, na placa. O quadro lido fica 4 a 5
  // vezes menor (1440p BGRA: 14,7 MB; 1080p NV12: 3,1 MB) e o navegador não converte mais nada no processador antes de
  // codificar. Medido sem isso numa RTX 4070 com Cyberpunk: 35 de 120 fps, 26 ms por quadro no codificador.
  struct Conversor {
    vc: ID3D11VideoContext1,
    vd: ID3D11VideoDevice,
    en: ID3D11VideoProcessorEnumerator,
    vp: ID3D11VideoProcessor,
    saida: ID3D11VideoProcessorOutputView,
    nv12: ID3D11Texture2D,
    leitura: ID3D11Texture2D,
    de: (u32, u32),
    para: (u32, u32),
  }

  impl Conversor {
    fn novo(quadro: &Frame, de: (u32, u32), para: (u32, u32)) -> windows::core::Result<Self> {
      let vd: ID3D11VideoDevice = quadro.device().cast()?;
      let vc: ID3D11VideoContext1 = quadro.device_context().cast()?;
      let taxa = DXGI_RATIONAL { Numerator: 60, Denominator: 1 };
      let cd = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: taxa,
        InputWidth: de.0,
        InputHeight: de.1,
        OutputFrameRate: taxa,
        OutputWidth: para.0,
        OutputHeight: para.1,
        Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
      };
      unsafe {
        let en = vd.CreateVideoProcessorEnumerator(&cd)?;
        let vp = vd.CreateVideoProcessor(&en, 0)?;
        let nv12 = textura(quadro.device(), para, DXGI_FORMAT_NV12, false)?;
        let leitura = textura(quadro.device(), para, DXGI_FORMAT_NV12, true)?;
        let od = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
          ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
          Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
        };
        let mut saida = None;
        vd.CreateVideoProcessorOutputView(&nv12, &en, &od, Some(&mut saida))?;
        // tela (RGB cheio) para vídeo BT.709 de faixa limitada, o que o VideoFrame da página declara
        vc.VideoProcessorSetStreamColorSpace1(&vp, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
        vc.VideoProcessorSetOutputColorSpace1(&vp, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709);
        vc.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, false);
        Ok(Self { vc, vd, en, vp, saida: saida.ok_or_else(windows::core::Error::empty)?, nv12, leitura, de, para })
      }
    }

    fn copiar(&self, quadro: &Frame, destino: &mut [u8]) -> windows::core::Result<()> {
      let id = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
        FourCC: 0,
        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 } },
      };
      unsafe {
        // o Windows gira as texturas da captura: a vista de entrada é do quadro da vez
        let mut entrada: Option<ID3D11VideoProcessorInputView> = None;
        self.vd.CreateVideoProcessorInputView(quadro.as_raw_texture(), &self.en, &id, Some(&mut entrada))?;
        let mut fluxo = D3D11_VIDEO_PROCESSOR_STREAM { Enable: true.into(), pInputSurface: ManuallyDrop::new(entrada), ..Default::default() };
        let r = self.vc.VideoProcessorBlt(&self.vp, &self.saida, 0, std::slice::from_ref(&fluxo));
        ManuallyDrop::drop(&mut fluxo.pInputSurface);
        r?;
        let ctx = quadro.device_context();
        ctx.CopyResource(&self.leitura, &self.nv12);
        ler(ctx, &self.leitura, self.para.0 as usize, (self.para.1 * 3 / 2) as usize, destino)
      }
    }
  }

  // tamanho de envio: cabe em `maximo` sem mudar a proporção, com lados pares (o NV12 pede)
  fn caber((w, h): (u32, u32), (mw, mh): (u32, u32)) -> (u32, u32) {
    let k = [1.0, if mw > 0 { mw as f64 / w as f64 } else { 1.0 }, if mh > 0 { mh as f64 / h as f64 } else { 1.0 }]
      .into_iter()
      .fold(f64::INFINITY, f64::min);
    (((w as f64 * k) as u32 & !1).max(2), ((h as f64 * k) as u32 & !1).max(2))
  }

  struct Envio {
    c: Arc<Compartilhado>,
    /// hora do próximo quadro a mandar
    proximo: Option<Instant>,
    conversor: Option<Conversor>,
    /// o processador de vídeo falhou nesta placa: segue em BGRA do tamanho da fonte
    sem_conversor: bool,
    /// textura que a CPU lê no caminho BGRA, reaproveitada
    leitura: Option<(ID3D11Texture2D, u32, u32)>,
  }

  impl Envio {
    // caminho BGRA: placa de vídeo -> memória compartilhada, do tamanho da fonte
    fn copiar_bgra(&mut self, quadro: &Frame, destino: &mut [u8]) -> Result<(), Erro> {
      let (w, h) = (quadro.width(), quadro.height());
      if !self.leitura.as_ref().is_some_and(|(_, lw, lh)| (*lw, *lh) == (w, h)) {
        self.leitura = Some((textura(quadro.device(), (w, h), quadro.desc().Format, true)?, w, h));
      }
      let (tex, _, _) = self.leitura.as_ref().unwrap();
      unsafe { quadro.device_context().CopyResource(tex, quadro.as_raw_texture()) };
      Ok(ler(quadro.device_context(), tex, (w * 4) as usize, h as usize, destino)?)
    }

    // prepara o conversor para este tamanho de fonte; None = vai em BGRA
    fn conversor(&mut self, quadro: &Frame) -> Option<&Conversor> {
      let de = (quadro.width(), quadro.height());
      if self.sem_conversor {
        return None;
      }
      if !self.conversor.as_ref().is_some_and(|c| c.de == de) {
        self.conversor = None;
        match Conversor::novo(quadro, de, caber(de, self.c.maximo)) {
          Ok(c) => self.conversor = Some(c),
          Err(e) => {
            eprintln!("captura: sem processador de vídeo ({e}), vai em BGRA");
            self.sem_conversor = true;
          }
        }
      }
      self.conversor.as_ref()
    }
  }

  impl GraphicsCaptureApiHandler for Envio {
    type Flags = Arc<Compartilhado>;
    type Error = Erro;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Erro> {
      Ok(Self { c: ctx.flags, proximo: None, conversor: None, sem_conversor: false, leitura: None })
    }

    fn on_frame_arrived(&mut self, quadro: &mut Frame, ctl: InternalCaptureControl) -> Result<(), Erro> {
      let c = self.c.clone();
      if c.parar.load(Ordering::Relaxed) {
        ctl.stop();
        return Ok(());
      }
      // o limite de quadros do Windows nem sempre existe (e deixa passar mais): segura aqui também. A hora do próximo
      // anda de intervalo em intervalo (a média fica no pedido), com 1 ms de folga para o quadro que chega um tiquinho
      // antes; atrasou demais (jogo minimizado, tela parada), recomeça a contar.
      let agora = Instant::now();
      if self.proximo.is_some_and(|p| agora + Duration::from_millis(1) < p) {
        return Ok(());
      }
      self.proximo = Some(match self.proximo {
        Some(p) if agora < p + c.intervalo => p + c.intervalo,
        _ => agora + c.intervalo,
      });
      let fonte = (quadro.width(), quadro.height());
      let (w, h, nv12) = match self.conversor(quadro) {
        Some(cv) => (cv.para.0, cv.para.1, true),
        None => (fonte.0, fonte.1, false),
      };
      let imagem = if nv12 { (w * h * 3 / 2) as usize } else { (w * h * 4) as usize };
      let bytes = CABECALHO + imagem;

      // tamanho novo (primeiro quadro, janela redimensionada, jogo trocou de resolução): buffers novos
      if c.bufs.lock().unwrap().tam != (w, h, nv12) {
        let novos = criar(&c.webview, bytes)?;
        let velhos = {
          let mut b = c.bufs.lock().unwrap();
          b.tam = (w, h, nv12);
          b.geracao += 1;
          std::mem::replace(&mut b.lista, novos)
        };
        c.soltar(velhos);
        c.tamanho.store(((w as u64) << 32) | h as u64, Ordering::Relaxed);
        c.fonte.store(((fonte.0 as u64) << 32) | fonte.1 as u64, Ordering::Relaxed);
        c.nv12.store(nv12, Ordering::Relaxed);
      }

      let (i, ptr, geracao) = {
        let mut b = c.bufs.lock().unwrap();
        let geracao = b.geracao;
        let livre = b.lista.iter().position(|x| {
          x.livre().load(Ordering::Acquire) == 0 || x.enviado.is_some_and(|t| agora - t > PRESO)
        });
        let Some(i) = livre else {
          c.descartados.fetch_add(1, Ordering::Relaxed);
          return Ok(());
        };
        b.lista[i].enviado = Some(agora);
        (i, b.lista[i].ptr, geracao)
      };

      let t0 = Instant::now();
      let destino = unsafe { std::slice::from_raw_parts_mut((ptr + CABECALHO) as *mut u8, imagem) };
      let copiado = match self.conversor.as_ref().filter(|_| nv12) {
        // a placa recusou a textura da captura: daqui em diante vai em BGRA (este quadro se perde)
        Some(cv) => cv.copiar(quadro, destino).or_else(|e| {
          eprintln!("captura: processador de vídeo falhou ({e}), vai em BGRA");
          self.sem_conversor = true;
          Ok(())
        }),
        None => self.copiar_bgra(quadro, destino),
      };
      if self.sem_conversor && nv12 {
        unsafe { &*(ptr as *const AtomicI32) }.store(0, Ordering::Release);
        return Ok(());
      }
      if let Err(e) = copiado {
        unsafe { &*(ptr as *const AtomicI32) }.store(0, Ordering::Release);
        return Err(e);
      }
      c.copia_us.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
      unsafe { &*(ptr as *const AtomicI32) }.store(1, Ordering::Release);

      // carimbo do quadro em microssegundos, no relógio do Windows (o mesmo para todos os quadros)
      let t = quadro.timestamp().map(|t| t.Duration / 10).unwrap_or_else(|_| c.inicio.elapsed().as_micros() as i64);
      let dados = HSTRING::from(format!(r#"{{"s":{},"i":{i},"w":{w},"h":{h},"t":{t},"o":{CABECALHO},"f":"{}"}}"#, c.sessao, if nv12 { "NV12" } else { "BGRX" }));
      let c2 = c.clone();
      let r = c.webview.with_webview(move |wv| {
        let b = c2.bufs.lock().unwrap();
        let enviado = (|| -> windows::core::Result<()> {
          if b.geracao != geracao {
            return Err(windows::core::Error::empty());
          }
          let com = b.lista.get(i).and_then(|x| x.com.as_ref()).ok_or_else(windows::core::Error::empty)?;
          let core: ICoreWebView2_17 = unsafe { wv.controller().CoreWebView2()? }.cast()?;
          unsafe { core.PostSharedBufferToScript(&com.0, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE, &dados) }
        })();
        match enviado {
          Ok(()) => {
            c2.quadros.fetch_add(1, Ordering::Relaxed);
          }
          // não foi: o buffer volta a ficar livre
          Err(_) => {
            if let Some(x) = b.lista.get(i).filter(|_| b.geracao == geracao) {
              x.livre().store(0, Ordering::Release);
            }
            c2.descartados.fetch_add(1, Ordering::Relaxed);
          }
        }
      });
      if r.is_err() {
        unsafe { &*(ptr as *const AtomicI32) }.store(0, Ordering::Release);
      }
      Ok(())
    }

    // a janela fechou (ou o jogo saiu): a página encerra a faixa
    fn on_closed(&mut self) -> Result<(), Erro> {
      let _ = self.c.eventos.send("fim".into());
      Ok(())
    }
  }

  pub struct Sessao {
    controle: Option<CaptureControl<Envio, Erro>>,
    c: Arc<Compartilhado>,
    alvo: String,
  }

  static PROXIMA: AtomicU32 = AtomicU32::new(1);

  impl Sessao {
    pub fn iniciar(webview: Webview, id: &str, fps: u32, maximo: (u32, u32), eventos: Channel<String>) -> Result<Self, String> {
      let alvo = Alvo::de(id)?;
      let c = Arc::new(Compartilhado {
        webview,
        sessao: PROXIMA.fetch_add(1, Ordering::Relaxed),
        intervalo: Duration::from_secs_f64(1.0 / fps as f64),
        maximo,
        parar: AtomicBool::new(false),
        bufs: Mutex::new(Buffers { tam: (0, 0, false), geracao: 0, lista: Vec::new() }),
        eventos,
        quadros: AtomicU32::new(0),
        descartados: AtomicU32::new(0),
        copia_us: AtomicU64::new(0),
        tamanho: AtomicU64::new(0),
        fonte: AtomicU64::new(0),
        nv12: AtomicBool::new(false),
        inicio: Instant::now(),
      });
      let controle = alvo.iniciar::<Envio>(true, Some(fps), c.clone())?;
      Ok(Self { controle: Some(controle), c, alvo: id.to_string() })
    }

    pub fn numero(&self) -> u32 {
      self.c.sessao
    }

    pub fn relatorio(&self) -> String {
      let c = &self.c;
      let (q, d) = (c.quadros.load(Ordering::Relaxed), c.descartados.load(Ordering::Relaxed));
      let (t, f) = (c.tamanho.load(Ordering::Relaxed), c.fonte.load(Ordering::Relaxed));
      let segundos = c.inicio.elapsed().as_secs_f32().max(0.001);
      let copia = c.copia_us.load(Ordering::Relaxed) as f32 / 1000.0 / q.max(1) as f32;
      format!(
        "{} · {}x{} saindo {} {}x{} · {q} quadros em {segundos:.0} s ({:.0} fps), {d} descartados, cópia {copia:.1} ms/quadro",
        self.alvo,
        f >> 32,
        f & 0xffff_ffff,
        if c.nv12.load(Ordering::Relaxed) { "NV12" } else { "BGRA" },
        t >> 32,
        t & 0xffff_ffff,
        q as f32 / segundos
      )
    }
  }

  impl Drop for Sessao {
    fn drop(&mut self) {
      self.c.parar.store(true, Ordering::Relaxed);
      if let Some(c) = self.controle.take() {
        let _ = c.stop();
      }
    }
  }
}
