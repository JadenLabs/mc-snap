use crate::orchestrate;
use crate::paths::ProjectLayout;
use crate::runtime::{process, rcon};
use crate::yml::Snap;
use anyhow::Result;

pub async fn run() -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    let pid = process::read_pid(&layout.pid_file());
    match pid {
        Some(p) if process::is_running_recorded(&layout.pid_file(), p) => {
            println!(
                "{}: {} (pid {p})",
                crate::style::bold(&snap.server.name),
                crate::style::green("running")
            );
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
            println!(
                "{}: {}",
                crate::style::bold(&snap.server.name),
                crate::style::dim("stopped")
            );
        }
    }
    Ok(())
}
