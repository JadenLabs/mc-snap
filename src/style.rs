//! Minimal ANSI styling helpers shared by every command.
//!
//! Styling is applied only when stdout is a terminal, `NO_COLOR` is unset, and
//! `TERM` is not "dumb". On Windows, virtual terminal processing is enabled
//! once on first use; if that fails (legacy console), styling is disabled so
//! users never see raw escape codes.

use std::io::IsTerminal;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
            return false;
        }
        if !std::io::stdout().is_terminal() {
            return false;
        }
        enable_windows_vt()
    })
}

#[cfg(windows)]
fn enable_windows_vt() -> bool {
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> isize;
        fn GetConsoleMode(hConsoleHandle: isize, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: isize, dwMode: u32) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const INVALID_HANDLE_VALUE: isize = -1;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
            return true;
        }
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_windows_vt() -> bool {
    true
}

fn wrap(code: &str, s: &str) -> String {
    if enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    wrap("1", s)
}

pub fn dim(s: &str) -> String {
    wrap("2", s)
}

pub fn cyan(s: &str) -> String {
    wrap("36", s)
}

pub fn green(s: &str) -> String {
    wrap("1;32", s)
}

pub fn yellow(s: &str) -> String {
    wrap("1;33", s)
}

pub fn red(s: &str) -> String {
    wrap("1;31", s)
}

/// Bold cyan, used for section headings like the init banner.
pub fn heading(s: &str) -> String {
    wrap("1;36", s)
}

/// Green check glyph prefixed to success lines.
pub fn ok() -> String {
    green("✓")
}

/// Yellow warning glyph.
pub fn warn_glyph() -> String {
    yellow("!")
}
