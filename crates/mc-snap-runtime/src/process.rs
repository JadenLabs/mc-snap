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

#[cfg(windows)]
pub fn is_running(pid: u32) -> bool {
    extern "system" {
        fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> isize;
        fn CloseHandle(hObject: isize) -> i32;
        fn GetExitCodeProcess(hProcess: isize, lpExitCode: *mut u32) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return false;
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        ok != 0 && exit_code == STILL_ACTIVE
    }
}

#[cfg(not(any(unix, windows)))]
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

#[cfg(windows)]
pub fn signal_term(pid: u32) -> Result<()> {
    // Windows has no SIGTERM equivalent; escalate directly to terminate.
    signal_kill(pid)
}

#[cfg(windows)]
pub fn signal_kill(pid: u32) -> Result<()> {
    extern "system" {
        fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> isize;
        fn TerminateProcess(hProcess: isize, uExitCode: u32) -> i32;
        fn CloseHandle(hObject: isize) -> i32;
    }
    const PROCESS_TERMINATE: u32 = 0x0001;
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        anyhow::ensure!(handle != 0, "OpenProcess failed for pid {pid}");
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub fn signal_term(_pid: u32) -> Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
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
