// App do Telão: uma janela que carrega o site. No Mac e no Windows acrescenta o que o navegador não dá (som na
// transmissão de tela, atalhos com o app em segundo plano, não lidas no ícone); no Android e no iOS é só o site.
// No Linux continua o Electron.

#[cfg(desktop)]
mod atualizar;
#[cfg(desktop)]
mod som;
#[cfg(desktop)]
mod teclas;

#[cfg(desktop)]
use tauri::{webview::NewWindowResponse, AppHandle, Manager, WebviewWindow};
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
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .manage(teclas::Estado::default())
    .manage(som::Estado::default())
    .invoke_handler(tauri::generate_handler![teclas::teclas, som::som, som::som_parar, diagnostico]);
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
        // Windows: o user agent só dá para mudar com o WebView2 criado; abre em branco, ajusta e então vai ao site
        let inicio = if cfg!(windows) { "about:blank".parse()? } else { site.clone() };
        let janela = WebviewWindowBuilder::new(app, "principal", WebviewUrl::External(inicio))
          .title("Telão")
          .inner_size(1280.0, 800.0)
          .background_color(tauri::window::Color(0x11, 0x12, 0x14, 0xff))
          .initialization_script(include_str!("ponte.js"))
          // Não lidas no ícone (Dock, barra de tarefas): o site já põe "(n) Telão" no título
          .on_document_title_changed(|janela, titulo| {
            let _ = janela.set_title(&titulo);
            contador(&janela, &titulo);
          })
          // fora do site (link de vídeo, documento): abre no navegador do sistema, nunca dentro do app
          .on_navigation(move |url| url.scheme() == "about" || url.origin() == origem || fora(url))
          .on_new_window(|url, _| {
            fora(&url);
            NewWindowResponse::Deny
          });
        #[cfg(target_os = "macos")]
        let janela = janela.user_agent(&format!("{UA_MAC} Telao/{}", app.package_info().version));
        let janela = janela.build()?;
        #[cfg(windows)]
        user_agent_windows(&janela, app.package_info().version.to_string(), site);
        #[cfg(not(debug_assertions))]
        atualizar::iniciar(app.handle().clone());
        let _ = janela;
      }
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("erro ao abrir o Telão")
    .run(|_, evento| {
      if let tauri::RunEvent::Exit = evento {
        #[cfg(all(desktop, not(debug_assertions)))]
        atualizar::ao_sair();
      }
    });
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

#[cfg(windows)]
fn user_agent_windows(janela: &WebviewWindow, versao: String, site: Url) {
  use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2;
  use windows::core::{Interface, HSTRING, PWSTR};
  let navegar = janela.clone();
  let _ = janela.with_webview(move |w| unsafe {
    let ajustar = || -> windows::core::Result<()> {
      let ajustes: ICoreWebView2Settings2 = w.controller().CoreWebView2()?.Settings()?.cast()?;
      let mut ua = PWSTR::null();
      ajustes.UserAgent(&mut ua)?;
      let atual = ua.to_string().unwrap_or_default();
      windows::Win32::System::Com::CoTaskMemFree(Some(ua.0 as _));
      ajustes.SetUserAgent(&HSTRING::from(format!("{atual} Telao/{versao}")))
    };
    if let Err(e) = ajustar() {
      eprintln!("user agent: {e}");
    }
    let _ = navegar.navigate(site);
  });
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
