// Atalhos e apertar para falar escolhidos no site, com o app em segundo plano. Só escuta o teclado (como o Discord):
// a tecla continua indo para o programa em uso. Responde se está escutando (senão o site cuida só com foco).
// O site manda no máximo 10 atalhos + 1 tecla de falar: não dá para virar um leitor de tudo que se digita.
// As teclas vêm como KeyboardEvent.code ("KeyM", "Digit1", "ControlLeft"); combinações como "Ctrl+Alt+KeyM".
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

#[derive(Deserialize)]
pub struct Escolhidas {
  atalhos: Vec<String>,
  ptt: Option<String>,
}

struct Atalho {
  combo: String,
  tecla: u32,
  mods: [bool; 4], // ctrl, alt, shift, meta
}

#[derive(Default)]
pub struct Estado(Mutex<Interno>);

#[derive(Default)]
struct Interno {
  canal: Option<Channel<Value>>,
  atalhos: Vec<Atalho>,
  ptt: Option<u32>,
  apertadas: HashSet<u32>,
  escutando: bool,
}

static APP: OnceLock<AppHandle> = OnceLock::new();

#[tauri::command]
pub fn teclas(app: AppHandle, estado: State<Estado>, escolhidas: Escolhidas, eventos: Channel<Value>) -> bool {
  let mut s = estado.0.lock().unwrap();
  s.canal = Some(eventos);
  s.atalhos = escolhidas
    .atalhos
    .iter()
    .take(10)
    .filter_map(|c| {
      let mut partes: Vec<&str> = c.split('+').collect();
      let tecla = plataforma::codigo(partes.pop()?)?;
      let tem = |m| partes.contains(&m);
      Some(Atalho { combo: c.clone(), tecla, mods: [tem("Ctrl"), tem("Alt"), tem("Shift"), tem("Meta")] })
    })
    .collect();
  s.ptt = escolhidas.ptt.as_deref().and_then(plataforma::codigo);
  if s.escutando {
    return true;
  }
  if s.atalhos.is_empty() && s.ptt.is_none() {
    return false;
  }
  let _ = APP.set(app);
  s.escutando = plataforma::escutar();
  s.escutando
}

// Chamado pelo gancho do sistema a cada tecla (de qualquer programa).
fn tecla(codigo: u32, desceu: bool) {
  let Some(app) = APP.get() else { return };
  let estado = app.state::<Estado>();
  let mut s = estado.0.lock().unwrap();
  if !desceu {
    s.apertadas.remove(&codigo);
    if s.ptt == Some(codigo) {
      envia(&s, json!({ "falar": false }));
    }
    return;
  }
  if !s.apertadas.insert(codigo) {
    return; // tecla segurada repete o keydown
  }
  if s.ptt == Some(codigo) {
    envia(&s, json!({ "falar": true }));
  }
  let mods = plataforma::MODIFICADORES.map(|lados| lados.iter().any(|m| s.apertadas.contains(m)));
  if let Some(a) = s.atalhos.iter().find(|a| a.tecla == codigo && a.mods == mods) {
    envia(&s, json!({ "atalho": a.combo }));
  }
}

fn envia(s: &Interno, evento: Value) {
  if let Some(c) = &s.canal {
    let _ = c.send(evento);
  }
}

#[cfg(target_os = "macos")]
mod plataforma {
  use std::ffi::c_void;

  // códigos virtuais do Mac (kVK_*): esquerdo e direito de ctrl, alt, shift, cmd
  pub const MODIFICADORES: [[u32; 2]; 4] = [[0x3B, 0x3E], [0x3A, 0x3D], [0x38, 0x3C], [0x37, 0x36]];

  pub fn codigo(code: &str) -> Option<u32> {
    let letras = "ASDFHGZXCVBQWERYT";
    let mapa: &[(&str, u32)] = &[
      ("Digit1", 0x12),
      ("Digit2", 0x13),
      ("Digit3", 0x14),
      ("Digit4", 0x15),
      ("Digit6", 0x16),
      ("Digit5", 0x17),
      ("Equal", 0x18),
      ("Digit9", 0x19),
      ("Digit7", 0x1A),
      ("Minus", 0x1B),
      ("Digit8", 0x1C),
      ("Digit0", 0x1D),
      ("BracketRight", 0x1E),
      ("KeyO", 0x1F),
      ("KeyU", 0x20),
      ("BracketLeft", 0x21),
      ("KeyI", 0x22),
      ("KeyP", 0x23),
      ("Enter", 0x24),
      ("KeyL", 0x25),
      ("KeyJ", 0x26),
      ("Quote", 0x27),
      ("KeyK", 0x28),
      ("Semicolon", 0x29),
      ("Backslash", 0x2A),
      ("Comma", 0x2B),
      ("Slash", 0x2C),
      ("KeyN", 0x2D),
      ("KeyM", 0x2E),
      ("Period", 0x2F),
      ("Tab", 0x30),
      ("Space", 0x31),
      ("Backquote", 0x32),
      ("Backspace", 0x33),
      ("Escape", 0x35),
      ("IntlBackslash", 0x0A),
      ("MetaRight", 0x36),
      ("MetaLeft", 0x37),
      ("ShiftLeft", 0x38),
      ("CapsLock", 0x39),
      ("AltLeft", 0x3A),
      ("ControlLeft", 0x3B),
      ("ShiftRight", 0x3C),
      ("AltRight", 0x3D),
      ("ControlRight", 0x3E),
      ("NumpadDecimal", 0x41),
      ("NumpadMultiply", 0x43),
      ("NumpadAdd", 0x45),
      ("NumLock", 0x47),
      ("NumpadDivide", 0x4B),
      ("NumpadEnter", 0x4C),
      ("NumpadSubtract", 0x4E),
      ("NumpadEqual", 0x51),
      ("Numpad0", 0x52),
      ("Numpad1", 0x53),
      ("Numpad2", 0x54),
      ("Numpad3", 0x55),
      ("Numpad4", 0x56),
      ("Numpad5", 0x57),
      ("Numpad6", 0x58),
      ("Numpad7", 0x59),
      ("Numpad8", 0x5B),
      ("Numpad9", 0x5C),
      ("IntlYen", 0x5D),
      ("IntlRo", 0x5E),
      ("NumpadComma", 0x5F),
      ("F1", 0x7A),
      ("F2", 0x78),
      ("F3", 0x63),
      ("F4", 0x76),
      ("F5", 0x60),
      ("F6", 0x61),
      ("F7", 0x62),
      ("F8", 0x64),
      ("F9", 0x65),
      ("F10", 0x6D),
      ("F11", 0x67),
      ("F12", 0x6F),
      ("F13", 0x69),
      ("F14", 0x6B),
      ("F15", 0x71),
      ("F16", 0x6A),
      ("F17", 0x40),
      ("F18", 0x4F),
      ("F19", 0x50),
      ("F20", 0x5A),
      ("Insert", 0x72),
      ("Home", 0x73),
      ("PageUp", 0x74),
      ("Delete", 0x75),
      ("End", 0x77),
      ("PageDown", 0x79),
      ("ArrowLeft", 0x7B),
      ("ArrowRight", 0x7C),
      ("ArrowDown", 0x7D),
      ("ArrowUp", 0x7E),
    ];
    if let Some(l) = code.strip_prefix("Key").filter(|l| l.len() == 1) {
      if let Some(i) = letras.find(l) {
        return Some(i as u32 + u32::from(i >= 10)); // 0x0A é a IntlBackslash, B começa em 0x0B
      }
    }
    mapa.iter().find(|(c, _)| *c == code).map(|(_, k)| *k)
  }

  // bits de cada lado dos modificadores nas flags do evento (NX_DEVICE*KEYMASK) e do Caps Lock
  const LADOS: [(u32, u64); 9] = [
    (0x3B, 0x1),
    (0x3E, 0x2000),
    (0x38, 0x2),
    (0x3C, 0x4),
    (0x37, 0x8),
    (0x36, 0x10),
    (0x3A, 0x20),
    (0x3D, 0x40),
    (0x39, 0x10000),
  ];

  #[link(name = "ApplicationServices", kind = "framework")]
  extern "C" {
    static kAXTrustedCheckOptionPrompt: *const c_void;
    fn AXIsProcessTrustedWithOptions(opcoes: *const c_void) -> bool;
    fn CGEventTapCreate(
      tap: u32,
      lugar: u32,
      opcoes: u32,
      mascara: u64,
      cb: extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void,
      info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, ligar: bool);
    fn CGEventGetIntegerValueField(evento: *mut c_void, campo: u32) -> i64;
    fn CGEventGetFlags(evento: *mut c_void) -> u64;
  }
  #[link(name = "CoreFoundation", kind = "framework")]
  extern "C" {
    static kCFBooleanTrue: *const c_void;
    static kCFRunLoopCommonModes: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDictionaryCreate(
      a: *const c_void,
      k: *const *const c_void,
      v: *const *const c_void,
      n: isize,
      kc: *const c_void,
      vc: *const c_void,
    ) -> *const c_void;
    fn CFRelease(p: *const c_void);
    fn CFMachPortCreateRunLoopSource(a: *const c_void, porta: *mut c_void, ordem: isize) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, fonte: *mut c_void, modo: *const c_void);
    fn CFRunLoopRun();
  }

  struct Tap(*mut c_void);
  unsafe impl Send for Tap {}
  unsafe impl Sync for Tap {}
  static TAP: std::sync::OnceLock<Tap> = std::sync::OnceLock::new();

  extern "C" fn recebe(_proxy: *mut c_void, tipo: u32, evento: *mut c_void, _info: *mut c_void) -> *mut c_void {
    unsafe {
      match tipo {
        // o sistema desliga o tap se ele demorar ou com a tela de senha: religa
        0xFFFF_FFFE | 0xFFFF_FFFF => {
          if let Some(t) = TAP.get() {
            CGEventTapEnable(t.0, true);
          }
        }
        10 | 11 => super::tecla(CGEventGetIntegerValueField(evento, 9) as u32, tipo == 10),
        12 => {
          let k = CGEventGetIntegerValueField(evento, 9) as u32;
          if let Some((_, bit)) = LADOS.iter().find(|(c, _)| *c == k) {
            super::tecla(k, CGEventGetFlags(evento) & bit != 0);
          }
        }
        _ => {}
      }
    }
    evento
  }

  pub fn escutar() -> bool {
    // precisa de Acessibilidade; a 1ª vez o sistema pede e só vale depois de reabrir o app
    unsafe {
      let k = [kAXTrustedCheckOptionPrompt];
      let v = [kCFBooleanTrue];
      let opcoes = CFDictionaryCreate(
        std::ptr::null(),
        k.as_ptr(),
        v.as_ptr(),
        1,
        &kCFTypeDictionaryKeyCallBacks as *const _ as _,
        &kCFTypeDictionaryValueCallBacks as *const _ as _,
      );
      let confia = AXIsProcessTrustedWithOptions(opcoes);
      CFRelease(opcoes);
      if !confia {
        return false;
      }
    }
    let (pronto, ok) = std::sync::mpsc::channel();
    std::thread::spawn(move || unsafe {
      // sessão, no início da fila, só escuta; keyDown, keyUp e flagsChanged
      let tap = CGEventTapCreate(1, 0, 1, (1 << 10) | (1 << 11) | (1 << 12), recebe, std::ptr::null_mut());
      let _ = pronto.send(!tap.is_null());
      if tap.is_null() {
        return;
      }
      let _ = TAP.set(Tap(tap));
      let fonte = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
      CFRunLoopAddSource(CFRunLoopGetCurrent(), fonte, kCFRunLoopCommonModes);
      CGEventTapEnable(tap, true);
      CFRunLoopRun();
    });
    ok.recv().unwrap_or(false)
  }
}

#[cfg(windows)]
mod plataforma {
  use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
  use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_SYSKEYDOWN,
  };

  // scan codes (conjunto 1; 0xE0xx = estendida): esquerdo e direito de ctrl, alt, shift, win
  pub const MODIFICADORES: [[u32; 2]; 4] = [[0x1D, 0xE01D], [0x38, 0xE038], [0x2A, 0x36], [0xE05B, 0xE05C]];

  pub fn codigo(code: &str) -> Option<u32> {
    let linhas = [("QWERTYUIOP", 0x10), ("ASDFGHJKL", 0x1E), ("ZXCVBNM", 0x2C)];
    if let Some(l) = code.strip_prefix("Key").filter(|l| l.len() == 1) {
      return linhas.iter().find_map(|(letras, base)| letras.find(l).map(|i| base + i as u32));
    }
    if let Some(d) = code.strip_prefix("Digit").and_then(|d| d.parse::<u32>().ok()) {
      return Some(if d == 0 { 0x0B } else { 0x01 + d });
    }
    if let Some(f) = code.strip_prefix('F').and_then(|f| f.parse::<u32>().ok()) {
      return match f {
        1..=10 => Some(0x3A + f),
        11 | 12 => Some(0x4C + f),
        13..=23 => Some(0x57 + f),
        24 => Some(0x76),
        _ => None,
      };
    }
    let mapa: &[(&str, u32)] = &[
      ("Escape", 0x01),
      ("Minus", 0x0C),
      ("Equal", 0x0D),
      ("Backspace", 0x0E),
      ("Tab", 0x0F),
      ("BracketLeft", 0x1A),
      ("BracketRight", 0x1B),
      ("Enter", 0x1C),
      ("ControlLeft", 0x1D),
      ("Semicolon", 0x27),
      ("Quote", 0x28),
      ("Backquote", 0x29),
      ("ShiftLeft", 0x2A),
      ("Backslash", 0x2B),
      ("Comma", 0x33),
      ("Period", 0x34),
      ("Slash", 0x35),
      ("ShiftRight", 0x36),
      ("NumpadMultiply", 0x37),
      ("AltLeft", 0x38),
      ("Space", 0x39),
      ("CapsLock", 0x3A),
      ("ScrollLock", 0x46),
      ("Numpad7", 0x47),
      ("Numpad8", 0x48),
      ("Numpad9", 0x49),
      ("NumpadSubtract", 0x4A),
      ("Numpad4", 0x4B),
      ("Numpad5", 0x4C),
      ("Numpad6", 0x4D),
      ("NumpadAdd", 0x4E),
      ("Numpad1", 0x4F),
      ("Numpad2", 0x50),
      ("Numpad3", 0x51),
      ("Numpad0", 0x52),
      ("NumpadDecimal", 0x53),
      ("IntlBackslash", 0x56),
      ("IntlRo", 0x73),
      ("NumpadEnter", 0xE01C),
      ("ControlRight", 0xE01D),
      ("NumpadDivide", 0xE035),
      ("PrintScreen", 0xE037),
      ("AltRight", 0xE038),
      ("NumLock", 0xE045),
      ("Home", 0xE047),
      ("ArrowUp", 0xE048),
      ("PageUp", 0xE049),
      ("ArrowLeft", 0xE04B),
      ("ArrowRight", 0xE04D),
      ("End", 0xE04F),
      ("ArrowDown", 0xE050),
      ("PageDown", 0xE051),
      ("Insert", 0xE052),
      ("Delete", 0xE053),
      ("MetaLeft", 0xE05B),
      ("MetaRight", 0xE05C),
      ("ContextMenu", 0xE05D),
    ];
    mapa.iter().find(|(c, _)| *c == code).map(|(_, k)| *k)
  }

  unsafe extern "system" fn recebe(n: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if n >= 0 {
      let k = &*(l.0 as *const KBDLLHOOKSTRUCT);
      // o Shift direito vem marcado como estendido, mas o scan code dele já é único
      let estendida = k.flags.contains(LLKHF_EXTENDED) && k.scanCode != 0x36;
      let codigo = k.scanCode | if estendida { 0xE000 } else { 0 };
      super::tecla(codigo, matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN));
    }
    CallNextHookEx(None, n, w, l)
  }

  pub fn escutar() -> bool {
    let (pronto, ok) = std::sync::mpsc::channel();
    // o gancho vale para a thread que o instalou, que precisa ficar tratando mensagens
    std::thread::spawn(move || unsafe {
      let gancho = SetWindowsHookExW(WH_KEYBOARD_LL, Some(recebe), None, 0);
      let _ = pronto.send(gancho.is_ok());
      if gancho.is_err() {
        return;
      }
      let mut msg = MSG::default();
      while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
    });
    ok.recv().unwrap_or(false)
  }
}
