// Captura própria de janela (Windows Graphics Capture, a base do Discord e do OBS), para o jogo em tela cheia que o
// seletor do WebView2 não lista. Passo 1: só mede (quadros, tamanho, custo de copiar cada quadro para a memória) e o
// resultado vai para o diagnóstico Ctrl+Shift+D. Passo 2: os quadros vão para o site e viram a faixa de vídeo da tela.

#[tauri::command(async)]
pub fn testar_captura(hwnd: String) -> String {
  #[cfg(windows)]
  return windows_teste::rodar(&hwnd);
  #[cfg(not(windows))]
  {
    let _ = hwnd;
    String::new()
  }
}

#[cfg(windows)]
mod windows_teste {
  use std::sync::{Arc, Mutex};
  use std::time::{Duration, Instant};
  use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
  use windows_capture::frame::Frame;
  use windows_capture::graphics_capture_api::InternalCaptureControl;
  use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings, SecondaryWindowSettings,
    Settings,
  };
  use windows_capture::window::Window;

  const DURACAO: Duration = Duration::from_secs(30);

  #[derive(Default)]
  struct Medida {
    quadros: u32,
    largura: u32,
    altura: u32,
    copia: Duration,
    primeiro: Option<Duration>,
  }

  struct Teste {
    medida: Arc<Mutex<Medida>>,
    inicio: Instant,
    memoria: Vec<u8>,
  }

  impl GraphicsCaptureApiHandler for Teste {
    type Flags = Arc<Mutex<Medida>>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
      Ok(Self { medida: ctx.flags, inicio: Instant::now(), memoria: Vec::new() })
    }

    fn on_frame_arrived(&mut self, quadro: &mut Frame, _: InternalCaptureControl) -> Result<(), Self::Error> {
      let t = Instant::now();
      let (largura, altura) = (quadro.width(), quadro.height());
      let buf = quadro.buffer()?;
      let _ = buf.as_nopadding_buffer(&mut self.memoria).len(); // a cópia da placa para a memória que o passo 2 vai precisar
      let mut m = self.medida.lock().unwrap();
      m.quadros += 1;
      m.copia += t.elapsed();
      (m.largura, m.altura) = (largura, altura);
      m.primeiro.get_or_insert(self.inicio.elapsed());
      Ok(())
    }
  }

  pub fn rodar(hwnd: &str) -> String {
    let Ok(h) = hwnd.parse::<usize>() else { return "hwnd inválido".into() };
    let medida = Arc::new(Mutex::new(Medida::default()));
    let config = Settings::new(
      Window::from_raw_hwnd(h as *mut std::ffi::c_void),
      CursorCaptureSettings::Default,
      DrawBorderSettings::WithoutBorder,
      SecondaryWindowSettings::Default,
      MinimumUpdateIntervalSettings::Default,
      DirtyRegionSettings::Default,
      ColorFormat::Bgra8,
      medida.clone(),
    );
    let controle = match Teste::start_free_threaded(config) {
      Ok(c) => c,
      Err(e) => return format!("não começou: {e:?}"),
    };
    std::thread::sleep(DURACAO);
    let parou = controle.stop().err().map(|e| format!(" (ao parar: {e:?})")).unwrap_or_default();
    let m = medida.lock().unwrap();
    if m.quadros == 0 {
      return format!("0 quadros em {} s{parou}", DURACAO.as_secs());
    }
    format!(
      "{} quadros em {} s, {}x{}, o primeiro com {:.1} s, cópia para a memória {:.1} ms/quadro{parou}",
      m.quadros,
      DURACAO.as_secs(),
      m.largura,
      m.altura,
      m.primeiro.unwrap_or_default().as_secs_f32(),
      m.copia.as_secs_f32() * 1000.0 / m.quadros as f32
    )
  }
}
