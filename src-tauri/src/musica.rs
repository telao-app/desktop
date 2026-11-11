// "Ouvindo X": o site pergunta (só com a opção ligada) e recebe "Artista - Música" do Spotify tocando, ou nada.
// Mac: pergunta ao próprio Spotify (o sistema pede a permissão uma vez). Windows: é o título da janela dele enquanto toca.

#[tauri::command(async)]
pub fn musica() -> Option<String> {
  tocando().map(|m| m.trim().to_string()).filter(|m| !m.is_empty())
}

#[cfg(target_os = "macos")]
fn tocando() -> Option<String> {
  // "is running" antes: sem ele o AppleScript abriria o Spotify
  const SCRIPT: &str = r#"if application "Spotify" is running then
tell application "Spotify" to if player state is playing then return artist of current track & " - " & name of current track
end if"#;
  let o = std::process::Command::new("osascript").args(["-e", SCRIPT]).output().ok()?;
  o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

#[cfg(windows)]
fn tocando() -> Option<String> {
  use windows::core::BOOL;
  use windows::Win32::Foundation::{HWND, LPARAM};
  use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId};
  unsafe extern "system" fn olhar(h: HWND, l: LPARAM) -> BOOL {
    unsafe {
      let mut t = [0u16; 512];
      let n = GetWindowTextW(h, &mut t) as usize;
      let titulo = String::from_utf16_lossy(&t[..n]);
      // parado o título é "Spotify Premium"/"Spotify Free"; as janelas internas não têm " - "
      if !titulo.contains(" - ") {
        return true.into();
      }
      let mut pid = 0u32;
      GetWindowThreadProcessId(h, Some(&mut pid));
      if !crate::jogo::caminho(pid).is_some_and(|c| c.to_ascii_lowercase().ends_with("\\spotify.exe")) {
        return true.into();
      }
      *(l.0 as *mut Option<String>) = Some(titulo);
      false.into()
    }
  }
  let mut achou: Option<String> = None;
  unsafe {
    let _ = EnumWindows(Some(olhar), LPARAM(&mut achou as *mut _ as isize));
  }
  achou
}
