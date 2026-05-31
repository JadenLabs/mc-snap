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
    write_pid_record(pid_file, pid)?;
    Ok(pid)
}

/// On-disk pid file format: `<pid>\n<start_token>`. The token is best-effort
/// per-OS — process start time on Windows (where pids recycle aggressively),
/// boot time on Unix (so a reboot invalidates the file). If we can't read a
/// token on this OS we fall through to plain pid-check, which matches the
/// previous behaviour.
fn write_pid_record(pid_file: &Path, pid: u32) -> Result<()> {
    let token = process_token(pid).unwrap_or_default();
    let body = if token.is_empty() {
        pid.to_string()
    } else {
        format!("{pid}\n{token}")
    };
    // Write to a temp sibling and rename so a crash mid-write can't leave a
    // half-written pid file that we then mis-parse.
    let tmp = pid_file.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, pid_file)?;
    Ok(())
}

pub fn read_pid(pid_file: &Path) -> Option<u32> {
    let s = std::fs::read_to_string(pid_file).ok()?;
    s.lines().next()?.trim().parse().ok()
}

fn read_token(pid_file: &Path) -> Option<String> {
    let s = std::fs::read_to_string(pid_file).ok()?;
    let mut it = s.lines();
    it.next()?; // pid
    it.next()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn clear_pid(pid_file: &Path) {
    std::fs::remove_file(pid_file).ok();
}

/// True iff the process is alive AND (on systems where we record a start token)
/// the token still matches — defends against pid reuse after a crash/reboot.
pub fn is_running_recorded(pid_file: &Path, pid: u32) -> bool {
    if !is_running(pid) {
        return false;
    }
    let saved = read_token(pid_file);
    let current = process_token(pid);
    match (saved, current) {
        (Some(a), Some(b)) => a == b,
        // No token recorded (legacy pid file, or platform without a usable token):
        // fall back to plain alive-check.
        (None, _) => true,
        // We have a saved token but the OS won't give us a current one — be safe and
        // assume identity matches (the process is at least alive).
        (Some(_), None) => true,
    }
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

/// A short per-process identity token used to detect pid reuse. None means we
/// can't compute one on this OS (caller falls back to plain alive-check).
#[cfg(windows)]
fn process_token(pid: u32) -> Option<String> {
    extern "system" {
        fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> isize;
        fn CloseHandle(hObject: isize) -> i32;
        fn GetProcessTimes(
            hProcess: isize,
            lpCreationTime: *mut FILETIME,
            lpExitTime: *mut FILETIME,
            lpKernelTime: *mut FILETIME,
            lpUserTime: *mut FILETIME,
        ) -> i32;
    }
    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    struct FILETIME {
        low: u32,
        high: u32,
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return None;
        }
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(format!("{}-{}", creation.high, creation.low))
    }
}

#[cfg(unix)]
fn process_token(pid: u32) -> Option<String> {
    // Parse start time (field 22) from /proc/<pid>/stat where available. The
    // comm field can contain spaces and parens, so split on the LAST ')'.
    let content = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = content.rsplit_once(')')?.1;
    let starttime = after_comm.split_whitespace().nth(19)?;
    Some(starttime.to_string())
}

#[cfg(not(any(unix, windows)))]
fn process_token(_pid: u32) -> Option<String> {
    None
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
