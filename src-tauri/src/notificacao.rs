// Notificação do sistema (Central de Notificações no Mac, aviso do Windows). Quem decide o que notificar é o site
// (menção, direta, sala silenciada, Não perturbe): a ponte troca a notificação do navegador por esta, que no Mac nem
// existe dentro do app. Clicar mostra a janela e avisa o site qual foi, para ele abrir a sala.
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

#[derive(Default)]
pub struct Estado(Mutex<Option<Channel<i32>>>);

// async: fora da thread da janela (no Windows, comando síncrono trava a janela)
#[tauri::command]
pub async fn notificar(app: AppHandle, estado: State<'_, Estado>, id: i32, titulo: String, corpo: String, cliques: Channel<i32>) -> Result<(), String> {
  *estado.0.lock().unwrap() = Some(cliques);
  // silenciosa: o site já toca o som dele
  app.notification().builder().id(id).title(titulo).body(corpo).silent().show().map_err(|e| e.to_string())
}

pub fn iniciar(app: &AppHandle) {
  let app2 = app.clone();
  // no desktop on_action nunca falha
  let _ = app.notification().on_action(move |a| {
    super::mostrar(&app2);
    let canal = app2.state::<Estado>().0.lock().unwrap().clone();
    if let (Some(c), Some(n)) = (canal, a.notification()) {
      let _ = c.send(n.id());
    }
  });
}
