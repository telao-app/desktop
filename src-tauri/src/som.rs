// Som que acompanha a transmissão de tela, entregue à página como PCM int16 estéreo 48 kHz (o ponte.js monta a faixa).
// Mac: o som do computador sem o que o próprio Telão toca (a voz dos colegas), por um tap do CoreAudio (macOS 14.2+).
// Windows: o som só do programa da janela escolhida, ou (tela inteira) o do computador menos o do Telão, pelo
// nativo/telao-som.exe (loopback por processo).
use std::sync::Mutex;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};

#[derive(Default)]
pub struct Estado(Mutex<Option<Captura>>);

// async: fora da thread principal (no Windows, comando síncrono trava a janela)
#[tauri::command]
pub async fn som(
  app: AppHandle,
  estado: State<'_, Estado>,
  alvo: String,
  pcm: Channel<InvokeResponseBody>,
) -> Result<(), String> {
  let mut atual = estado.0.lock().unwrap();
  *atual = None; // encerra a anterior antes de abrir outra
  *atual = Some(Captura::iniciar(&app, &alvo, pcm)?);
  Ok(())
}

#[tauri::command]
pub async fn som_parar(estado: State<'_, Estado>) -> Result<(), String> {
  *estado.0.lock().unwrap() = None;
  Ok(())
}

#[cfg(windows)]
pub use windows_som::janelas;
#[cfg(windows)]
use windows_som::Captura;

#[cfg(target_os = "macos")]
use mac::Captura;

#[cfg(windows)]
mod windows_som {
  use super::*;
  use std::io::Read;
  use std::os::windows::process::CommandExt;
  use std::process::{Child, Command, Stdio};
  use tauri::Manager;

  const SEM_JANELA: u32 = 0x0800_0000; // CREATE_NO_WINDOW: sem console piscando

  pub struct Captura(Child);

  impl Captura {
    // alvo: o hwnd da janela (o som só do programa dela), ou "sistema" (tudo menos o próprio Telão: tela inteira)
    pub fn iniciar(app: &AppHandle, alvo: &str, pcm: Channel<InvokeResponseBody>) -> Result<Self, String> {
      let args = if alvo == "sistema" {
        vec!["--sem".to_string(), std::process::id().to_string()]
      } else {
        vec![alvo.parse::<u64>().map_err(|_| "janela inválida")?.to_string()]
      };
      let exe = app.path().resource_dir().map_err(|e| e.to_string())?.join("telao-som.exe");
      let mut filho = Command::new(exe)
        .args(args)
        .stdout(Stdio::piped())
        .creation_flags(SEM_JANELA)
        .spawn()
        .map_err(|e| format!("telao-som: {e}"))?;
      let mut saida = filho.stdout.take().unwrap();
      std::thread::spawn(move || {
        // o pipe corta em qualquer byte; manda só quadros inteiros (4 bytes: 2 canais de 16 bits)
        let mut resto = Vec::new();
        let mut buf = [0u8; 16384];
        while let Ok(n) = saida.read(&mut buf) {
          if n == 0 {
            break;
          }
          resto.extend_from_slice(&buf[..n]);
          let inteiro = resto.len() - resto.len() % 4;
          if inteiro > 0 && pcm.send(InvokeResponseBody::Raw(resto.drain(..inteiro).collect())).is_err() {
            break;
          }
        }
      });
      Ok(Captura(filho))
    }
  }

  impl Drop for Captura {
    fn drop(&mut self) {
      let _ = self.0.kill();
      let _ = self.0.wait();
    }
  }

  // janelas que o Windows lista (o seletor do WebView2 esconde jogo em tela cheia): só para o diagnóstico
  pub fn janelas(app: &AppHandle) -> Vec<String> {
    let Ok(dir) = app.path().resource_dir() else { return vec![] };
    let exe = dir.join("telao-som.exe");
    match Command::new(exe).arg("--janelas").creation_flags(SEM_JANELA).output() {
      Ok(s) => String::from_utf8_lossy(&s.stdout).lines().filter_map(|l| l.split('\t').nth(2).map(str::to_string)).collect(),
      Err(e) => vec![format!("erro: {e}")],
    }
  }
}

#[cfg(target_os = "macos")]
mod mac {
  use super::*;
  use objc2::msg_send;
  use objc2::rc::Retained;
  use objc2::runtime::{AnyClass, AnyObject};
  use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};
  use std::ffi::c_void;
  use std::sync::mpsc;

  const fn cc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
  }
  const SISTEMA: u32 = 1;
  const GLOBAL: u32 = cc(b"glob");

  #[repr(C)]
  struct Endereco {
    seletor: u32,
    escopo: u32,
    elemento: u32,
  }
  #[repr(C)]
  #[derive(Default, Clone, Copy)]
  struct Formato {
    taxa: f64,
    id: u32,
    flags: u32,
    bytes_pacote: u32,
    quadros_pacote: u32,
    bytes_quadro: u32,
    canais: u32,
    bits: u32,
    reservado: u32,
  }
  #[repr(C)]
  struct Buffer {
    canais: u32,
    bytes: u32,
    dados: *const c_void,
  }
  #[repr(C)]
  struct ListaBuffers {
    n: u32,
    buffers: [Buffer; 1],
  }
  type IoProc =
    extern "C" fn(u32, *const c_void, *const ListaBuffers, *const c_void, *mut c_void, *const c_void, *mut c_void) -> i32;

  #[link(name = "CoreAudio", kind = "framework")]
  extern "C" {
    fn AudioObjectGetPropertyDataSize(obj: u32, end: *const Endereco, q: u32, qd: *const c_void, tam: *mut u32) -> i32;
    fn AudioObjectGetPropertyData(
      obj: u32,
      end: *const Endereco,
      q: u32,
      qd: *const c_void,
      tam: *mut u32,
      dados: *mut c_void,
    ) -> i32;
    fn AudioHardwareCreateProcessTap(desc: *const AnyObject, tap: *mut u32) -> i32;
    fn AudioHardwareDestroyProcessTap(tap: u32) -> i32;
    fn AudioHardwareCreateAggregateDevice(desc: *const AnyObject, dev: *mut u32) -> i32;
    fn AudioHardwareDestroyAggregateDevice(dev: u32) -> i32;
    fn AudioDeviceCreateIOProcID(dev: u32, proc_: IoProc, dados: *mut c_void, id: *mut *mut c_void) -> i32;
    fn AudioDeviceDestroyIOProcID(dev: u32, id: *mut c_void) -> i32;
    fn AudioDeviceStart(dev: u32, id: *mut c_void) -> i32;
    fn AudioDeviceStop(dev: u32, id: *mut c_void) -> i32;
  }
  extern "C" {
    // quem "responde" por um processo: os processos do WebKit (som da página) respondem pelo app
    fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32;
  }

  fn ler<T: Copy + Default>(obj: u32, seletor: u32) -> Option<T> {
    let end = Endereco { seletor, escopo: GLOBAL, elemento: 0 };
    let mut v = T::default();
    let mut tam = std::mem::size_of::<T>() as u32;
    let r = unsafe { AudioObjectGetPropertyData(obj, &end, 0, std::ptr::null(), &mut tam, &mut v as *mut T as *mut c_void) };
    (r == 0).then_some(v)
  }

  // os objetos de áudio dos processos deste app (ele mesmo e os do WebKit, que tocam a voz da sala)
  fn processos_do_app() -> Vec<u32> {
    let end = Endereco { seletor: cc(b"prs#"), escopo: GLOBAL, elemento: 0 };
    let mut tam = 0u32;
    if unsafe { AudioObjectGetPropertyDataSize(SISTEMA, &end, 0, std::ptr::null(), &mut tam) } != 0 {
      return vec![];
    }
    let mut ids = vec![0u32; tam as usize / 4];
    if unsafe { AudioObjectGetPropertyData(SISTEMA, &end, 0, std::ptr::null(), &mut tam, ids.as_mut_ptr() as *mut c_void) } != 0 {
      return vec![];
    }
    let eu = std::process::id() as i32;
    ids
      .into_iter()
      .filter(|&id| {
        ler::<i32>(id, cc(b"ppid"))
          .is_some_and(|pid| pid == eu || unsafe { responsibility_get_pid_responsible_for_pid(pid) } == eu)
      })
      .collect()
  }

  fn uid_da_saida() -> Option<Retained<NSString>> {
    let dev: u32 = ler(SISTEMA, cc(b"dOut"))?;
    let uid: *mut NSString = ler::<usize>(dev, cc(b"uid "))? as *mut NSString;
    unsafe { Retained::from_raw(uid) } // a propriedade devolve um CFString nosso
  }

  struct Contexto {
    formato: Formato,
    envia: mpsc::Sender<Vec<f32>>,
  }

  // thread de tempo real do CoreAudio: só copia para estéreo intercalado e passa adiante
  extern "C" fn recebe(
    _: u32,
    _: *const c_void,
    entrada: *const ListaBuffers,
    _: *const c_void,
    _: *mut c_void,
    _: *const c_void,
    dados: *mut c_void,
  ) -> i32 {
    let ctx = unsafe { &*(dados as *const Contexto) };
    let lista = unsafe { &*entrada };
    let bufs = unsafe { std::slice::from_raw_parts(lista.buffers.as_ptr(), lista.n as usize) };
    let Some(b0) = bufs.first() else { return 0 };
    if b0.dados.is_null() {
      return 0;
    }
    let separado = ctx.formato.flags & (1 << 5) != 0; // kAudioFormatFlagIsNonInterleaved
    let amostras = |b: &Buffer| unsafe { std::slice::from_raw_parts(b.dados as *const f32, b.bytes as usize / 4) };
    let mut saida;
    if separado {
      let esq = amostras(b0);
      let dir = bufs.get(1).filter(|b| !b.dados.is_null()).map(amostras).unwrap_or(esq);
      saida = Vec::with_capacity(esq.len() * 2);
      for (e, d) in esq.iter().zip(dir) {
        saida.extend_from_slice(&[*e, *d]);
      }
    } else {
      let c = b0.canais.max(1) as usize;
      let a = amostras(b0);
      saida = Vec::with_capacity(a.len() / c * 2);
      for q in a.chunks_exact(c) {
        saida.extend_from_slice(&[q[0], q[if c > 1 { 1 } else { 0 }]]);
      }
    }
    let _ = ctx.envia.send(saida);
    0
  }

  pub struct Captura {
    tap: u32,
    dev: u32,
    proc_: *mut c_void,
    _ctx: Box<Contexto>,
  }
  unsafe impl Send for Captura {}

  impl Captura {
    pub fn iniciar(_app: &AppHandle, alvo: &str, pcm: Channel<InvokeResponseBody>) -> Result<Self, String> {
      if alvo != "sistema" {
        return Err("alvo inválido".into());
      }
      let classe = AnyClass::get(c"CATapDescription").ok_or("o som do computador precisa do macOS 14.2 ou mais novo")?;
      let excluir: Vec<Retained<NSNumber>> = processos_do_app().into_iter().map(NSNumber::new_u32).collect();
      let excluir = NSArray::from_retained_slice(&excluir);
      unsafe {
        let desc: Retained<AnyObject> = msg_send![msg_send![classe, alloc], initStereoGlobalTapButExcludeProcesses: &*excluir];
        let _: () = msg_send![&*desc, setPrivate: true];
        let uuid: Retained<NSUUID> = msg_send![&*desc, UUID];
        let mut tap = 0u32;
        let r = AudioHardwareCreateProcessTap(&*desc, &mut tap);
        if r != 0 {
          return Err(format!("sem permissão para gravar o som do computador ({r})"));
        }
        let formato: Formato = ler(tap, cc(b"tfmt")).unwrap_or_default();
        let sub_tap = NSDictionary::<NSString, AnyObject>::from_slices(
          &[&*NSString::from_str("uid"), &*NSString::from_str("drift")],
          &[&*uuid.UUIDString(), &*NSNumber::new_bool(true)],
        );
        let taps = NSArray::from_retained_slice(&[sub_tap]);
        let mut chaves = vec![
          NSString::from_str("name"),
          NSString::from_str("uid"),
          NSString::from_str("private"),
          NSString::from_str("tapautostart"),
          NSString::from_str("taps"),
        ];
        let mut valores: Vec<Retained<AnyObject>> = vec![
          Retained::into_super(Retained::into_super(NSString::from_str("Telão som"))),
          Retained::into_super(Retained::into_super(NSUUID::new().UUIDString())),
          Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_bool(true)))),
          Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_bool(true)))),
          Retained::into_super(Retained::into_super(taps)),
        ];
        // o relógio do dispositivo agregado vem da saída de som atual
        if let Some(saida) = uid_da_saida() {
          let sub = NSDictionary::<NSString, AnyObject>::from_slices(&[&*NSString::from_str("uid")], &[&*saida]);
          chaves.push(NSString::from_str("master"));
          valores.push(Retained::into_super(Retained::into_super(saida)));
          chaves.push(NSString::from_str("subdevices"));
          valores.push(Retained::into_super(Retained::into_super(NSArray::from_retained_slice(&[sub]))));
        }
        let k: Vec<&NSString> = chaves.iter().map(|c| &**c).collect();
        let v: Vec<&AnyObject> = valores.iter().map(|c| &**c).collect();
        let agregado = NSDictionary::<NSString, AnyObject>::from_slices(&k, &v);
        let mut dev = 0u32;
        let r = AudioHardwareCreateAggregateDevice(&*agregado as *const _ as *const AnyObject, &mut dev);
        if r != 0 {
          AudioHardwareDestroyProcessTap(tap);
          return Err(format!("dispositivo de som ({r})"));
        }
        let (envia, recebe_) = mpsc::channel::<Vec<f32>>();
        let ctx = Box::new(Contexto { formato, envia });
        let mut proc_ = std::ptr::null_mut();
        let r = AudioDeviceCreateIOProcID(dev, recebe, &*ctx as *const Contexto as *mut c_void, &mut proc_);
        if r != 0 || AudioDeviceStart(dev, proc_) != 0 {
          AudioHardwareDestroyAggregateDevice(dev);
          AudioHardwareDestroyProcessTap(tap);
          return Err(format!("iniciar o som ({r})"));
        }
        std::thread::spawn(move || entregar(recebe_, formato.taxa, pcm));
        Ok(Captura { tap, dev, proc_, _ctx: ctx })
      }
    }
  }

  impl Drop for Captura {
    fn drop(&mut self) {
      unsafe {
        AudioDeviceStop(self.dev, self.proc_);
        AudioDeviceDestroyIOProcID(self.dev, self.proc_);
        AudioHardwareDestroyAggregateDevice(self.dev);
        AudioHardwareDestroyProcessTap(self.tap);
      }
    }
  }

  // leva para 48 kHz (a saída pode estar em 44,1), converte para int16 e manda em pedaços de ~20 ms
  fn entregar(chega: mpsc::Receiver<Vec<f32>>, taxa: f64, pcm: Channel<InvokeResponseBody>) {
    let passo = if taxa > 0.0 { taxa / 48_000.0 } else { 1.0 };
    let mut pos = 0.0f64;
    let mut anterior = [0f32; 2];
    let mut pedaco: Vec<u8> = Vec::with_capacity(3840 * 2);
    for bloco in chega {
      let n = bloco.len() / 2;
      while pos < n as f64 {
        let i = pos.floor() as usize;
        let f = (pos - i as f64) as f32;
        let a = if i == 0 { anterior } else { [bloco[(i - 1) * 2], bloco[(i - 1) * 2 + 1]] };
        let b = [bloco[i * 2], bloco[i * 2 + 1]];
        for c in 0..2 {
          let s = (a[c] + (b[c] - a[c]) * f).clamp(-1.0, 1.0);
          pedaco.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
        }
        pos += passo;
      }
      pos -= n as f64;
      if n > 0 {
        anterior = [bloco[(n - 1) * 2], bloco[(n - 1) * 2 + 1]];
      }
      if pedaco.len() >= 3840 && pcm.send(InvokeResponseBody::Raw(std::mem::take(&mut pedaco))).is_err() {
        return;
      }
    }
  }
}
