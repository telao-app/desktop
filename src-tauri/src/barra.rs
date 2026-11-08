// Windows: o WebView2 põe uma barra flutuante "<site> está compartilhando uma janela" (Parar compartilhamento / Ocultar)
// enquanto a tela é transmitida, e não tem opção para tirar. O Telão já mostra que está transmitindo e tem o botão de
// parar: quando a captura começa, acha a janela dessa barra (título com o endereço do site, baixinha) e esconde.
// ponytail: casa pelo título; se o WebView2 mudar o texto, a barra só volta a aparecer.

#[tauri::command(async)]
pub fn esconder_barra(webview: tauri::Webview) {
  #[cfg(windows)]
  if let Some(host) = webview.url().ok().and_then(|u| u.host_str().map(str::to_string)) {
    windows_barra::esconder(&host);
  }
  #[cfg(not(windows))]
  let _ = webview;
}

#[cfg(windows)]
mod windows_barra {
  use std::time::{Duration, Instant};
  use windows::core::BOOL;
  use windows::Win32::Foundation::{HWND, LPARAM, RECT};
  use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, GetWindowTextW, IsWindowVisible, ShowWindowAsync, SW_HIDE};

  const ALTURA_MAX: i32 = 240; // px físicos: a barra tem ~50 e a escala do Windows vai até 300%

  unsafe extern "system" fn juntar(hwnd: HWND, l: LPARAM) -> BOOL {
    (*(l.0 as *mut Vec<HWND>)).push(hwnd);
    true.into()
  }

  fn eh_a_barra(hwnd: HWND, host: &str) -> bool {
    unsafe {
      let mut r = RECT::default();
      if !IsWindowVisible(hwnd).as_bool() || GetWindowRect(hwnd, &mut r).is_err() {
        return false;
      }
      let altura = r.bottom - r.top;
      let mut buf = [0u16; 256];
      let n = GetWindowTextW(hwnd, &mut buf) as usize;
      altura > 0 && altura <= ALTURA_MAX && String::from_utf16_lossy(&buf[..n]).contains(host)
    }
  }

  // A barra aparece logo depois da captura começar: procura por alguns segundos.
  pub fn esconder(host: &str) {
    let fim = Instant::now() + Duration::from_secs(10);
    while Instant::now() < fim {
      let mut janelas: Vec<HWND> = Vec::new();
      unsafe {
        let _ = EnumWindows(Some(juntar), LPARAM(&mut janelas as *mut _ as isize));
      }
      if let Some(&barra) = janelas.iter().find(|&&h| eh_a_barra(h, host)) {
        unsafe {
          let _ = ShowWindowAsync(barra, SW_HIDE); // async: o processo do navegador ocupado não segura esta thread
        }
        return;
      }
      std::thread::sleep(Duration::from_millis(150));
    }
  }
}

// Windows: a janela interna do single-instance ("<identificador>-siw", invisível na tela) aparecia como um quadro preto no
// seletor de janelas. Janela de ferramenta não entra na lista.
#[cfg(windows)]
pub fn esconder_janela_interna(identificador: &str) {
  use windows::core::HSTRING;
  use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW};
  unsafe {
    if let Ok(h) = FindWindowW(None, &HSTRING::from(format!("{identificador}-siw"))) {
      let estilo = GetWindowLongPtrW(h, GWL_EXSTYLE);
      SetWindowLongPtrW(h, GWL_EXSTYLE, estilo | WS_EX_TOOLWINDOW.0 as isize);
    }
  }
}
