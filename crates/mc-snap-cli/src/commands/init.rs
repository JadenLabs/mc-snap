use anyhow::Result;
use std::path::Path;

const TEMPLATE: &str = r#"schema: 1
eula: false

server:
  name: my-server
  description: a mc-snap server
  minecraft: 1.21.4
  loader:
    type: fabric

runtime:
  java: 21
  memory: 4G
  flags:
    - -XX:+UseG1GC

mods:
  - id: fabric-api
    provider: modrinth
    version: latest

config:
  server.properties:
    motd: my-server
    max-players: 20
"#;

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let target = cwd.join("mc-snap.yml");
    if target.exists() {
        anyhow::bail!("mc-snap.yml already exists at {}", target.display());
    }
    std::fs::write(&target, TEMPLATE)?;
    ensure_gitignore(&cwd)?;
    println!("created {}", target.display());
    println!("edit it, set `eula: true` after reading https://www.minecraft.net/en-us/eula, then run `mc-snap install`");
    Ok(())
}

fn ensure_gitignore(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".mc-snap") || existing.lines().any(|l| l.trim() == ".mc-snap/") {
        return Ok(());
    }
    let mut new = existing;
    if !new.ends_with('\n') && !new.is_empty() {
        new.push('\n');
    }
    new.push_str(".mc-snap/\n");
    std::fs::write(path, new)?;
    Ok(())
}
