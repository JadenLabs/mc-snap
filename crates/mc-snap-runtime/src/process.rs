use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

pub async fn run_foreground(mut cmd: Command) -> Result<i32> {
    let mut child = cmd.spawn().context("spawning server")?;
    let status = child.wait().await?;
    Ok(status.code().unwrap_or(-1))
}

pub fn spawn_detached(mut cmd: Command, pid_file: &Path) -> Result<u32> {
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().pre_exec(|| {
            nix::unistd::setsid().ok();
            Ok(())
        });
    }
    let child = cmd.spawn().context("spawning detached server")?;
    let pid = child.id().ok_or_else(|| anyhow::anyhow!("no pid"))?;
    std::fs::write(pid_file, pid.to_string())?;
    Ok(pid)
}

pub fn read_pid(pid_file: &Path) -> Option<u32> {
    let s = std::fs::read_to_string(pid_file).ok()?;
    s.trim().parse().ok()
}

pub fn clear_pid(pid_file: &Path) {
    std::fs::remove_file(pid_file).ok();
}

#[cfg(unix)]
pub fn is_running(pid: u32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(not(unix))]
pub fn is_running(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
pub fn signal_term(pid: u32) -> Result<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM)?;
    Ok(())
}

#[cfg(unix)]
pub fn signal_kill(pid: u32) -> Result<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), Signal::SIGKILL)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn signal_term(_pid: u32) -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
pub fn signal_kill(_pid: u32) -> Result<()> {
    Ok(())
}

pub async fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !is_running(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    !is_running(pid)
}
