use crate::orchestrate;
use anyhow::Result;
use crate::paths::ProjectLayout;
use crate::yml::Snap;
use crate::runtime::{process, rcon};

pub async fn run() -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    let pid = process::read_pid(&layout.pid_file());
    match pid {
        Some(p) if process::is_running(p) => {
            println!("{}: running (pid {p})", snap.server.name);
            if let Ok(pw) = orchestrate::read_rcon_password(&layout) {
                let addr = orchestrate::rcon_address(&snap);
                if let Ok(mut r) = rcon::Rcon::connect(&addr, &pw).await {
                    if let Ok(out) = r.exec("list").await {
                        println!("  {}", out.trim());
                    }
                }
            }
        }
        _ => {
            println!("{}: stopped", snap.server.name);
        }
    }
    Ok(())
}
