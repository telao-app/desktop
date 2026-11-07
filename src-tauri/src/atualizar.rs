#![cfg_attr(debug_assertions, allow(dead_code))] // só roda no app empacotado
                                                 // Baixa a versão nova em segundo plano e pergunta se reinicia agora; "Depois" instala ao fechar o app.
                                                 // Mac: troca só o Contents do próprio Telao.app (a pasta fica a mesma e o atalho do Dock continua valendo). Sem o
                                                 // atualizador do Tauri de propósito: o Electron instalado hoje baixa o mesmo Telao-mac.zip e vira este app sozinho.
                                                 // Windows: roda o instalador novo em silêncio depois que o app fecha (e reabre, se pediu para reiniciar).
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

const RELEASES: &str = "https://api.github.com/repos/telao-app/telao-app-desktop/releases/latest";
#[cfg(target_os = "macos")]
const ARQUIVO: &str = "Telao-mac.zip";
#[cfg(windows)]
const ARQUIVO: &str = "Telao-windows.exe";

#[derive(Deserialize)]
struct Release {
  tag_name: String,
  assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
  name: String,
  browser_download_url: String,
}

// o que fica pronto para rodar quando o app fechar
struct Pendente {
  novo: PathBuf,
  reabrir: bool,
}
static PENDENTE: Mutex<Option<Pendente>> = Mutex::new(None);

pub fn iniciar(app: AppHandle) {
  tauri::async_runtime::spawn(async move {
    loop {
      if let Err(e) = checar(&app).await {
        eprintln!("atualização: {e}");
      }
      if PENDENTE.lock().unwrap().is_some() {
        return;
      }
      tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
    }
  });
}

fn partes(v: &str) -> Vec<u64> {
  v.trim_start_matches('v').split(['.', '-']).take(3).map(|n| n.parse().unwrap_or(0)).collect()
}

async fn checar(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
  let cliente = reqwest::Client::builder().user_agent(format!("Telao/{}", app.package_info().version)).build()?;
  let ultima: Release = cliente.get(RELEASES).send().await?.error_for_status()?.json().await?;
  if partes(&ultima.tag_name) <= partes(&app.package_info().version.to_string()) {
    return Ok(());
  }
  let asset = ultima.assets.iter().find(|a| a.name == ARQUIVO).ok_or("release sem o instalador")?;
  if !da_para_trocar() {
    // aberto do dmg ou de pasta sem permissão: só avisa e leva para o download
    let url = asset.browser_download_url.replace(".zip", ".dmg");
    let pergunta = app.dialog().message(format!("Versão nova do Telão: {}", ultima.tag_name));
    pergunta.buttons(MessageDialogButtons::OkCancelCustom("Baixar".into(), "Depois".into())).show(move |baixar| {
      if baixar {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
      }
    });
    *PENDENTE.lock().unwrap() = Some(Pendente { novo: PathBuf::new(), reabrir: false });
    return Ok(());
  }
  let dir = std::env::temp_dir().join(format!("telao-{}", ultima.tag_name));
  std::fs::create_dir_all(&dir)?;
  let arquivo = dir.join(ARQUIVO);
  let bytes = cliente.get(&asset.browser_download_url).send().await?.error_for_status()?.bytes().await?;
  std::fs::write(&arquivo, &bytes)?;
  #[cfg(target_os = "macos")]
  let novo = {
    let ok = std::process::Command::new("ditto").args(["-x", "-k"]).arg(&arquivo).arg(&dir).status()?;
    if !ok.success() {
      return Err("ditto falhou".into());
    }
    dir.join("Telao.app")
  };
  #[cfg(windows)]
  let novo = arquivo;
  *PENDENTE.lock().unwrap() = Some(Pendente { novo, reabrir: false });
  perguntar(app, &ultima.tag_name);
  Ok(())
}

fn perguntar(app: &AppHandle, versao: &str) {
  let sair = app.clone();
  app
    .dialog()
    .message("Reinicie para usar a versão nova (leva poucos segundos). Se preferir, ela entra sozinha quando você fechar o app.")
    .title(format!("Telão {} pronto", versao.trim_start_matches('v')))
    .buttons(MessageDialogButtons::OkCancelCustom("Reiniciar agora".into(), "Depois".into()))
    .show(move |agora| {
      if agora {
        if let Some(p) = PENDENTE.lock().unwrap().as_mut() {
          p.reabrir = true;
        }
        sair.exit(0);
      }
    });
}

#[cfg(target_os = "macos")]
fn app_atual() -> Option<PathBuf> {
  // .../Telao.app/Contents/MacOS/telao
  std::env::current_exe().ok()?.ancestors().nth(3).map(PathBuf::from)
}

fn da_para_trocar() -> bool {
  #[cfg(target_os = "macos")]
  {
    let Some(pasta) = app_atual().and_then(|a| a.parent().map(PathBuf::from)) else { return false };
    let teste = pasta.join(".telao-escrita");
    let ok = std::fs::write(&teste, b"").is_ok();
    let _ = std::fs::remove_file(teste);
    ok
  }
  #[cfg(windows)]
  true
}

/// Chamado quando o app fecha: troca a versão depois que este processo sair.
pub fn ao_sair() {
  let Some(p) = PENDENTE.lock().unwrap().take() else { return };
  if p.novo.as_os_str().is_empty() {
    return;
  }
  let pid = std::process::id().to_string();
  let reabrir = if p.reabrir { "1" } else { "0" };
  #[cfg(target_os = "macos")]
  {
    use std::os::unix::process::CommandExt;
    let Some(atual) = app_atual() else { return };
    // O Contents antigo vai para a pasta temporária (o sistema limpa) e volta se a troca falhar.
    let _ = std::process::Command::new("/bin/sh")
      .args([
        "-c",
        r#"while kill -0 "$0" 2>/dev/null; do sleep 0.2; done; mv "$1/Contents" "$2.velho" && { mv "$2/Contents" "$1/Contents" || mv "$2.velho" "$1/Contents"; }; [ "$3" = 1 ] && open "$1""#,
        &pid,
      ])
      .arg(&atual)
      .arg(&p.novo)
      .arg(reabrir)
      .process_group(0)
      .spawn();
  }
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return };
    let script = format!(
      "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; Start-Process -Wait -FilePath '{}' -ArgumentList '/S'; if ('{reabrir}' -eq '1') {{ Start-Process -FilePath '{}' }}",
      p.novo.display().to_string().replace('\'', "''"),
      exe.display().to_string().replace('\'', "''")
    );
    let _ = std::process::Command::new("powershell")
      .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
      .creation_flags(0x0000_0008 | 0x0800_0000) // DETACHED_PROCESS | CREATE_NO_WINDOW
      .spawn();
  }
}
