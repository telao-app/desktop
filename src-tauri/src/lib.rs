// App do Telão: uma janela que carrega o site. No Mac e no Windows acrescenta o que o navegador não dá (som na
// transmissão de tela, atalhos com o app em segundo plano, não lidas no ícone); no Android e no iOS é só o site.
// No Linux continua o Electron.

#[cfg(desktop)]
mod abrir;
#[cfg(desktop)]
mod atualizar;
#[cfg(desktop)]
mod barra;
#[cfg(desktop)]
mod jogo;
#[cfg(desktop)]
mod notificacao;
#[cfg(desktop)]
mod som;
#[cfg(desktop)]
mod teclas;

#[cfg(desktop)]
use tauri::{webview::{NewWindowResponse, PermissionKind, PermissionResponse}, AppHandle, Manager, WebviewWindow};
use tauri::{Url, WebviewUrl, WebviewWindowBuilder};
#[cfg(desktop)]
use tauri_plugin_clipboard_manager::ClipboardExt;
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

// ponytail: carrega o site; quando o front for empacotado aqui dentro, deixa de depender do servidor entregar o código
const URL_PADRAO: &str = "https://telao.nyxdev.com.br";

// O WebKit do Mac tem um user agent fixo; o site mostra a versão em "Sobre" lendo "Telao/<versão>" dele
#[cfg(target_os = "macos")]
const UA_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let app = tauri::Builder::default().plugin(tauri_plugin_opener::init());
  #[cfg(desktop)]
  let app = app
    // Abrir o Telão de novo (menu Iniciar, atalho) com ele escondido: mostra a janela que já existe, sem segunda cópia
    .plugin(tauri_plugin_single_instance::init(|app, _, _| mostrar(app)))
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![abrir::ESCONDIDO])))
    .manage(notificacao::Estado::default())
    .plugin(tauri_plugin_clipboard_manager::init())
    .manage(teclas::Estado::default())
    .manage(som::Estado::default())
    .invoke_handler(tauri::generate_handler![teclas::teclas, som::som, som::som_parar, jogo::jogo, jogo::jogos_minimizados, jogo::perguntar_jogo, notificacao::notificar, abrir::abrir_com_pc, atualizar::atualizacao, atualizar::atualizar, barra::esconder_barra, diagnostico]);
  app
    .setup(|app| {
      let site: Url = std::env::var("TELAO_URL").unwrap_or_else(|_| URL_PADRAO.into()).parse()?;
      let origem = site.origin();
      #[cfg(mobile)]
      {
        // celular: só o site, com os links de fora abrindo no navegador do sistema
        WebviewWindowBuilder::new(app, "principal", WebviewUrl::External(site))
          .on_navigation(move |url| url.origin() == origem || fora(url))
          .build()?;
      }
      #[cfg(desktop)]
      {
        let janela = WebviewWindowBuilder::new(app, "principal", WebviewUrl::External(site))
          .title("Telão")
          // aberto junto com o computador: começa escondido, já conectado
          .visible(!std::env::args().any(|a| a == abrir::ESCONDIDO))
          .inner_size(1280.0, 800.0)
          .background_color(tauri::window::Color(0x11, 0x12, 0x14, 0xff))
          .initialization_script(include_str!("ponte.js"))
          // Não lidas no ícone (Dock, barra de tarefas): o site já põe "(n) Telão" no título
          .on_document_title_changed(|janela, titulo| {
            let _ = janela.set_title(&titulo);
            contador(&janela, &titulo);
          })
          // fora do site (link de vídeo, documento): abre no navegador do sistema, nunca dentro do app
          .on_navigation(move |url| url.origin() == origem || fora(url))
          .on_new_window(|url, _| {
            fora(&url);
            NewWindowResponse::Deny
          })
          // A janela só abre o site do Telão: microfone e câmera liberados sem o aviso do WebView2, que, uma vez
          // recusado, fica gravado e nunca mais pergunta (a voz não funciona e reinstalar não limpa)
          .on_permission_request(|_, tipo| match tipo {
            PermissionKind::Microphone | PermissionKind::Camera => PermissionResponse::Allow,
            _ => PermissionResponse::Default,
          });
        #[cfg(target_os = "macos")]
        let janela = janela.user_agent(&format!("{UA_MAC} Telao/{}", app.package_info().version));
        #[cfg(windows)]
        let janela = janela.user_agent(&ua_windows(&app.package_info().version.to_string()));
        let janela = janela.build()?;
        // O X esconde (como o Discord): o app segue conectado e avisando. Sair de vez: ⌘Q no Mac, "Sair" no ícone do Windows.
        let escondida = janela.clone();
        janela.on_window_event(move |e| {
          if let tauri::WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            let _ = escondida.hide();
          }
        });
        #[cfg(windows)]
        icone_bandeja(app)?;
        #[cfg(windows)]
        if let Ok(h) = janela.hwnd() {
          barra::esconder_janela_interna(&app.config().identifier, windows::Win32::Foundation::HWND(h.0));
        }
        notificacao::iniciar(app.handle());
        abrir::iniciar(app.handle());
        #[cfg(not(debug_assertions))]
        atualizar::iniciar(app.handle().clone());
        let _ = janela;
      }
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("erro ao abrir o Telão")
    .run(|_app, evento| match evento {
      tauri::RunEvent::Exit => {
        #[cfg(all(desktop, not(debug_assertions)))]
        atualizar::ao_sair();
      }
      // Mac: clicar no ícone do Dock com a janela escondida
      #[cfg(target_os = "macos")]
      tauri::RunEvent::Reopen { .. } => mostrar(_app),
      _ => {}
    });
}

#[cfg(desktop)]
fn mostrar(app: &AppHandle) {
  if let Some(j) = app.get_webview_window("principal") {
    let _ = j.unminimize();
    let _ = j.show();
    let _ = j.set_focus();
  }
}

// Windows: ícone perto do relógio. Clicar abre a janela; o menu tem "Sair", que fecha de verdade.
#[cfg(windows)]
fn icone_bandeja(app: &tauri::App) -> tauri::Result<()> {
  use tauri::menu::{Menu, MenuItem};
  use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
  let abrir = MenuItem::with_id(app, "abrir", "Abrir Telão", true, None::<&str>)?;
  let sair = MenuItem::with_id(app, "sair", "Sair", true, None::<&str>)?;
  let mut icone = TrayIconBuilder::new()
    .tooltip("Telão")
    .menu(&Menu::with_items(app, &[&abrir, &sair])?)
    .show_menu_on_left_click(false)
    .on_menu_event(|app, e| match e.id.as_ref() {
      "abrir" => mostrar(app),
      "sair" => app.exit(0),
      _ => {}
    })
    .on_tray_icon_event(|t, e| {
      if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
        mostrar(t.app_handle());
      }
    });
  if let Some(i) = app.default_window_icon() {
    icone = icone.icon(i.clone());
  }
  icone.build(app)?;
  Ok(())
}

// Roda dentro do evento de navegação do webview: abrir o navegador ali mesmo segura a janela, então vai numa thread.
fn fora(url: &Url) -> bool {
  if matches!(url.scheme(), "http" | "https" | "mailto") {
    let url = url.to_string();
    std::thread::spawn(move || tauri_plugin_opener::open_url(url, None::<&str>));
  }
  false
}

// Mac mostra o número; o Windows só aceita um ícone sobreposto, então vai uma bolinha vermelha
#[cfg(desktop)]
fn contador(janela: &WebviewWindow, titulo: &str) {
  let n = titulo.strip_prefix('(').and_then(|r| r.split(')').next()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
  #[cfg(not(windows))]
  let _ = janela.set_badge_count((n > 0).then_some(n));
  #[cfg(windows)]
  let _ = janela.set_overlay_icon((n > 0).then(bolinha));
}

#[cfg(windows)]
fn bolinha() -> tauri::image::Image<'static> {
  const T: usize = 16;
  let mut px = vec![0u8; T * T * 4];
  for i in 0..T * T {
    let (x, y) = ((i % T) as f32 - 7.5, (i / T) as f32 - 7.5);
    if x * x + y * y <= 56.0 {
      px[i * 4..i * 4 + 4].copy_from_slice(&[0xed, 0x42, 0x45, 0xff]); // RGBA, vermelho
    }
  }
  tauri::image::Image::new_owned(px, T as u32, T as u32)
}

// WebView2 é o Edge: o user agent dele + "Telao/<versão>", já no builder (antes era em branco, troca e navega, e a
// janela ficava preta no Windows)
#[cfg(windows)]
fn ua_windows(versao: &str) -> String {
  let edge = tauri::webview_version().unwrap_or_else(|_| "130.0.0.0".into());
  let maior = edge.split('.').next().unwrap_or("130");
  format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{maior}.0.0.0 Safari/537.36 Edg/{edge} Telao/{versao}")
}

// Ctrl+Shift+D (⌘⇧D no Mac): o que a minha tela está enviando e por que está limitada
#[cfg(desktop)]
#[tauri::command]
async fn diagnostico(app: AppHandle, texto: String) {
  #[allow(unused_mut)]
  let mut relatorio = format!("Telão {} · {}\n{texto}", app.package_info().version, std::env::consts::OS);
  #[cfg(windows)]
  relatorio.push_str(&format!("\nJanelas (Windows): {}", som::janelas(&app).join(" | ")));
  let copiar = app.clone();
  let janela = app.get_webview_window("principal");
  let mut caixa = app
    .dialog()
    .message(relatorio.clone())
    .title("Diagnóstico da transmissão")
    .buttons(MessageDialogButtons::OkCancelCustom("Copiar".into(), "Fechar".into()));
  if let Some(j) = &janela {
    caixa = caixa.parent(j);
  }
  caixa.show(move |copiou| {
    if copiou {
      let _ = copiar.clipboard().write_text(relatorio);
    }
  });
}
