// "Jogando X": o site pergunta a cada minuto (só com a opção ligada) e recebe o nome do jogo aberto, ou nada.
// Reconhece pela pasta onde as lojas instalam (Steam, Epic, GOG, Riot, EA, Ubisoft, Xbox): o nome da pasta é o do jogo.
// Só o nome sai daqui; a lista de programas abertos nunca vai para o site.
#[cfg(not(windows))]
use std::process::Command;

const LOJAS: &[&str] = &[
  "/steamapps/common/",
  "/epic games/",
  "/gog galaxy/games/",
  "/riot games/",
  "/ea games/",
  "/ubisoft game launcher/games/",
  "/xboxgames/",
];
const NAO_JOGOS: &[&str] =
  &["launcher", "epic online services", "directxredist", "riot client", "steamworks shared", "steam controller configs"];

// async: ler os processos não pode travar a janela
#[tauri::command(async)]
pub fn jogo() -> Option<String> {
  caminhos().iter().find_map(|c| nome(c))
}

// também marca os jogos no seletor próprio de tela (captura.rs)
pub(crate) fn nome(caminho: &str) -> Option<String> {
  let c = caminho.replace('\\', "/");
  let baixo = c.to_ascii_lowercase(); // mesmo tamanho em bytes: o índice vale para o original
  LOJAS.iter().find_map(|loja| {
    let i = baixo.find(loja)? + loja.len();
    let pasta = c.get(i..)?.split('/').next()?;
    (!pasta.is_empty() && !NAO_JOGOS.contains(&pasta.to_ascii_lowercase().as_str())).then(|| pasta.to_string())
  })
}

// Mac: `ps`. Windows: a lista de processos do sistema com o caminho de cada um. Antes era o `Get-Process` do PowerShell,
// que não mostra o caminho de programa rodando com mais permissão (jogo com anticheat, aberto pelo Ubisoft Connect):
// o Far Cry 6 da Steam não aparecia. O acesso "limitado" do QueryFullProcessImageNameW enxerga esses também.
#[cfg(not(windows))]
fn caminhos() -> Vec<String> {
  Command::new("ps")
    .args(["-axo", "comm="])
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect())
    .unwrap_or_default()
}

#[cfg(windows)]
fn caminhos() -> Vec<String> {
  use windows::Win32::Foundation::CloseHandle;
  use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
  let mut saida = Vec::new();
  unsafe {
    let Ok(lista) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return saida };
    let mut p = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut tem = Process32FirstW(lista, &mut p).is_ok();
    while tem {
      saida.extend(caminho(p.th32ProcessID));
      tem = Process32NextW(lista, &mut p).is_ok();
    }
    let _ = CloseHandle(lista);
  }
  saida
}

#[cfg(windows)]
pub(crate) fn caminho(pid: u32) -> Option<String> {
  use windows::core::PWSTR;
  use windows::Win32::Foundation::CloseHandle;
  use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
  unsafe {
    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 1024];
    let mut n = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
    let _ = CloseHandle(h);
    ok.then(|| String::from_utf16_lossy(&buf[..n as usize]))
  }
}

#[cfg(test)]
mod testes {
  use super::nome;

  #[test]
  fn reconhece_pela_pasta_da_loja() {
    assert_eq!(nome(r"C:\Program Files (x86)\Steam\steamapps\common\Hades II\Hades2.exe").as_deref(), Some("Hades II"));
    assert_eq!(nome(r"C:\Program Files (x86)\Steam\steamapps\common\Far Cry 6\bin\FarCry6.exe").as_deref(), Some("Far Cry 6"));
    assert_eq!(nome(r"D:\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient.exe").as_deref(), Some("Fortnite"));
    assert_eq!(
      nome("/Users/a/Library/Application Support/Steam/steamapps/common/Celeste/Celeste.app/Contents/MacOS/Celeste").as_deref(),
      Some("Celeste")
    );
    assert_eq!(nome(r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"), None);
    assert_eq!(nome(r"C:\Riot Games\Riot Client\RiotClientServices.exe"), None);
    assert_eq!(nome(r"C:\Windows\explorer.exe"), None);
  }
}
