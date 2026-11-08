fn main() {
  // Comandos que o site (origem remota) pode chamar: cada um vira uma permissão "allow-<comando>" (capabilities/site.json)
  tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
    "teclas",
    "som",
    "som_parar",
    "diagnostico",
    "jogo",
    "jogos_minimizados",
    "perguntar_jogo",
    "notificar",
    "abrir_com_pc",
    "atualizacao",
    "atualizar",
    "esconder_barra",
  ])))
  .expect("tauri-build");
}
