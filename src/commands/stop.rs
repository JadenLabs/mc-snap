use crate::orchestrate;
use anyhow::Result;
use crate::paths::ProjectLayout;
use crate::proclock::ProjectLock;
use crate::yml::Snap;
use crate::runtime::{process, rcon};
use std::time::Duration;
use tracing::warn;

pub async fn run() -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    std::fs::create_dir_all(layout.snap_dir())?;
    let _guard = ProjectLock::acquire(&layout.lock_file())?;
    let snap = Snap::from_path(&layout.yml())?;

    let pid = process::read_pid(&layout.pid_file());
    match pid {
        Some(p) if !process::is_running_recorded(&layout.pid_file(), p) => {
            process::clear_pid(&layout.pid_file());
            println!("server not running");
            return Ok(());
        }
        None => {
            println!("server not running");
            return Ok(());
        }
        _ => {}
    }
    let pid = pid.unwrap();

    let addr = orchestrate::rcon_address(&snap);
    let pw = orchestrate::read_rcon_password(&layout)?;
    match rcon::Rcon::connect(&addr, &pw).await {
        Ok(mut r) => {
            let _ = r.exec("stop").await;
        }
        Err(e) => warn!("rcon stop failed: {e}; falling back to signals"),
    }

    if process::wait_for_exit(pid, Duration::from_secs(20)).await {
        process::clear_pid(&layout.pid_file());
        println!("stopped");
        return Ok(());
    }

    warn!("sending SIGTERM to {pid}");
    process::signal_term(pid)?;
    if process::wait_for_exit(pid, Duration::from_secs(10)).await {
        process::clear_pid(&layout.pid_file());
        println!("stopped (term)");
        return Ok(());
    }

    warn!("sending SIGKILL to {pid}");
    process::signal_kill(pid)?;
    process::clear_pid(&layout.pid_file());
    println!("stopped (kill)");
    Ok(())
}
