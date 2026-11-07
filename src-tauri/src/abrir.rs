// Abrir o Telão ao ligar o computador, já escondido (ícone perto do relógio no Windows, Dock no Mac), como o Discord.
// Vem ligado: na primeira vez que esta versão abre, liga e marca que já decidiu; depois vale o que a pessoa escolher no site.
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

pub const ESCONDIDO: &str = "--escondido";

pub fn iniciar(app: &AppHandle) {
  if cfg!(debug_assertions) {
    return; // o app de desenvolvimento não se registra para abrir com o computador
  }
  let Ok(dir) = app.path().app_config_dir() else { return };
  let marca = dir.join("abrir-com-o-computador");
  if !marca.exists() && app.autolaunch().enable().is_ok() {
    let _ = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&marca, ""));
  }
}

// O site liga/desliga (Configurações) e lê como está. async: fora da thread da janela.
#[tauri::command(async)]
pub fn abrir_com_pc(app: AppHandle, ligar: Option<bool>) -> bool {
  let a = app.autolaunch();
  match ligar {
    Some(true) => drop(a.enable()),
    Some(false) => drop(a.disable()),
    None => {}
  }
  a.is_enabled().unwrap_or(false)
}
