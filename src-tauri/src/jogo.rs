// "Jogando X": o site pergunta a cada minuto (só com a opção ligada) e recebe o nome do jogo aberto, ou nada.
// Reconhece pela pasta onde as lojas instalam (Steam, Epic, GOG, Riot, EA, Ubisoft, Xbox): o nome da pasta é o do jogo.
// Só o nome sai daqui; a lista de programas abertos nunca vai para o site.
use std::process::Command;

const LOJAS: &[&str] = &["/steamapps/common/", "/epic games/", "/gog galaxy/games/", "/riot games/", "/ea games/", "/ubisoft game launcher/games/", "/xboxgames/"];
const NAO_JOGOS: &[&str] = &["launcher", "epic online services", "directxredist", "riot client", "steamworks shared", "steam controller configs"];

// async: o PowerShell leva ~1 s e não pode travar a janela
#[tauri::command(async)]
pub fn jogo() -> Option<String> {
  caminhos().iter().find_map(|c| nome(c))
}

fn nome(caminho: &str) -> Option<String> {
  let c = caminho.replace('\\', "/");
  let baixo = c.to_ascii_lowercase(); // mesmo tamanho em bytes: o índice vale para o original
  LOJAS.iter().find_map(|loja| {
    let i = baixo.find(loja)? + loja.len();
    let pasta = c.get(i..)?.split('/').next()?;
    (!pasta.is_empty() && !NAO_JOGOS.contains(&pasta.to_ascii_lowercase().as_str())).then(|| pasta.to_string())
  })
}

// ponytail: um processo (ps/PowerShell) por minuto; trocar pela API do sistema se pesar
fn caminhos() -> Vec<String> {
  #[cfg(windows)]
  let saida = {
    use std::os::windows::process::CommandExt;
    Command::new("powershell")
      .args(["-NoProfile", "-Command", "Get-Process | ForEach-Object Path"])
      .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: sem janela de console piscando
      .output()
  };
  #[cfg(not(windows))]
  let saida = Command::new("ps").args(["-axo", "comm="]).output();
  saida.map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect()).unwrap_or_default()
}

#[cfg(test)]
mod testes {
  use super::nome;

  #[test]
  fn reconhece_pela_pasta_da_loja() {
    assert_eq!(nome(r"C:\Program Files (x86)\Steam\steamapps\common\Hades II\Hades2.exe").as_deref(), Some("Hades II"));
    assert_eq!(nome(r"D:\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient.exe").as_deref(), Some("Fortnite"));
    assert_eq!(nome("/Users/a/Library/Application Support/Steam/steamapps/common/Celeste/Celeste.app/Contents/MacOS/Celeste").as_deref(), Some("Celeste"));
    assert_eq!(nome(r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"), None);
    assert_eq!(nome(r"C:\Riot Games\Riot Client\RiotClientServices.exe"), None);
    assert_eq!(nome(r"C:\Windows\explorer.exe"), None);
  }
}
