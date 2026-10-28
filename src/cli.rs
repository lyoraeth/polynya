// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Executable commands: without arguments the island starts; with a command it is a client that
//! sends a message to the running island and exits.
//!
//! ```text
//! polynya push <title> [details] [--key K] [--ttl SEC] [--icon ok|fail|info|key|claude|agent|color:#rrggbb] [--meta TEXT]
//! polynya dismiss <K>
//! polynya refresh
//! polynya notify-setup
//! polynya modules reload
//! polynya modules enable|disable <id>...
//! polynya language ru|en|zh|ja
//! polynya quit
//! polynya uninstall [--purge]
//! ```

use windows::Win32::System::{
    Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
    },
    SystemInformation::GetLocalTime,
};

use proto::Msg;

use crate::{config, i18n};

/// Default notification lifetime, s.
const DEFAULT_TTL: u64 = 10;
/// Longer details are truncated: both the chip and the expanded view fit a couple of lines.
const MAX_BODY: usize = 160;

const USAGE: &str = "\
usage:
  polynya push <title> [details] [--key K] [--ttl SEC] [--icon ok|fail|info|key|claude|agent|color:#rrggbb] [--meta TEXT]
      --ttl 0  stays until closed; default is 10 s
      --key    a notification with the same key replaces the previous one
  polynya dismiss <K>
  polynya refresh       recapture the taskbar look (after patches like Windhawk)
  polynya notify-setup  install the Windows notifications module package (asks for admin rights)
  polynya modules reload  restart module processes and pick up changed modules
  polynya modules enable|disable <id>...  turn modules on or off (applies on the next start)
  polynya language ru|en|zh|ja  set the interface language (applies on the next start)
  polynya quit          close the island
  polynya uninstall [--purge]  close the island and undo its system changes (autostart, agent
      hooks, notifications package); --purge also deletes the settings and logs
  polynya help";

/// Runs the command, if any. `None`: no arguments, the island itself starts.
pub fn run() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first()?.as_str();
    // A relaunch from the settings is the island itself.
    if cmd == "--relaunch" {
        return None;
    }
    i18n::init(config::load().language);
    Some(match cmd {
        "help" | "--help" | "-h" | "/?" => {
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            println!("{}\n\nPolynya {}\n\n{USAGE}", logo(color_console()), env!("POLYNYA_VERSION"));
            0
        }
        "push" | "dismiss" => {
            // A windowed app has no console of its own; write to the console of whoever launched it.
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            let parsed =
                if cmd == "push" { push(&args[1..]) } else { args.get(1).map(|k| Msg::Dismiss { key: k.clone() }).ok_or_else(|| "a key is required".into()) };
            match parsed.and_then(|m| proto::send(&m)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("polynya: {e}\n{USAGE}");
                    1
                }
            }
        }
        "refresh" => {
            use windows::Win32::{
                Foundation::{LPARAM, WPARAM},
                UI::WindowsAndMessaging::{FindWindowW, PostMessageW},
            };
            // SAFETY: both strings are static and null-terminated.
            match unsafe { FindWindowW(windows::core::w!("Polynya"), windows::core::w!("Polynya")) } {
                Ok(hwnd) => {
                    // SAFETY: posting a message to a window handle has no memory-safety preconditions.
                    let _ = unsafe { PostMessageW(Some(hwnd), crate::WM_APP_REFRESH, WPARAM(0), LPARAM(0)) };
                    0
                }
                Err(_) => {
                    // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
                    unsafe {
                        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
                    }
                    eprintln!("polynya: the island is not running");
                    1
                }
            }
        }
        "modules" if args.get(1).map(String::as_str) == Some("reload") => {
            use windows::Win32::{
                Foundation::{LPARAM, WPARAM},
                UI::WindowsAndMessaging::{FindWindowW, PostMessageW},
            };
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            // SAFETY: both strings are static and null-terminated.
            match unsafe { FindWindowW(windows::core::w!("Polynya"), windows::core::w!("Polynya")) } {
                Ok(hwnd) => {
                    // SAFETY: posting a message to a window handle has no memory-safety preconditions.
                    let _ = unsafe { PostMessageW(Some(hwnd), crate::WM_APP_MODULES, WPARAM(0), LPARAM(0)) };
                    println!("polynya: modules reloaded, see the log for details");
                    0
                }
                Err(_) => {
                    eprintln!("polynya: the island is not running");
                    1
                }
            }
        }
        "modules" if matches!(args.get(1).map(String::as_str), Some("enable" | "disable")) => {
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            set_modules(args[1] == "enable", &args[2..])
        }
        "language" => {
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            match args.get(1).filter(|code| i18n::Lang::parse(code).is_some()) {
                Some(code) => {
                    config::set("language", code.trim());
                    0
                }
                None => {
                    eprintln!("polynya: expected a language: ru, en, zh or ja");
                    1
                }
            }
        }
        "quit" => {
            crate::install::stop(crate::CLASS, crate::ipc::INSTANCE_MUTEX);
            0
        }
        "uninstall" => {
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            uninstall(args.iter().any(|a| a == "--purge"))
        }
        "notify-setup" => {
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            notify_setup()
        }
        _ => {
            // SAFETY: `AttachConsole` has no memory-safety preconditions; failure is ignored.
            unsafe {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
            eprintln!("{USAGE}");
            2
        }
    })
}

/// Turns the given modules on or off. Fails on an unknown id without changing anything: the
/// installer passes ids of the bundled modules, so a mismatch is a packaging mistake.
fn set_modules(on: bool, ids: &[String]) -> i32 {
    let known: Vec<String> = crate::modules::scan().into_iter().map(|m| m.id).collect();
    if let Some(bad) = ids.iter().find(|id| !known.contains(id)) {
        eprintln!("polynya: unknown module {bad}");
        return 1;
    }
    for id in ids {
        crate::modules::set_enabled(id, on);
    }
    0
}

/// Closes the island and undoes what it registered in the system, so the files can be deleted
/// cleanly. Called by the uninstaller; `purge` also deletes the settings and logs.
fn uninstall(purge: bool) -> i32 {
    crate::install::stop(crate::CLASS, crate::ipc::INSTANCE_MUTEX);
    // Only our own entry: another copy (portable or installed) may own the autostart.
    if crate::autostart::is_enabled() {
        crate::autostart::set(false);
    }
    // Before purging: a module may restore system settings from a backup kept in the data folder.
    crate::modules::teardown();
    if purge {
        for dir in [Some(proto::data_dir()), proto::log_dir()].into_iter().flatten() {
            if let Err(e) = std::fs::remove_dir_all(&dir)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("polynya: delete {}: {e}", dir.display());
                return 1;
            }
        }
    }
    0
}

/// Installs the notifications module package. Without admin rights it relaunches itself with them
/// (UAC prompt), while the island itself stays unelevated. When done, the island starts the module.
fn notify_setup() -> i32 {
    let Some(folder) = crate::modules::dir().map(|d| d.join("notify")) else { return 1 };
    if crate::package::elevated() {
        if let Err(e) = crate::package::setup(&folder) {
            proto::error!("notify", "setup: {e}");
            eprintln!("polynya: {e}");
            return 1;
        }
    } else if !crate::package::run_elevated("notify-setup") {
        eprintln!("polynya: admin rights were not granted");
        return 1;
    }
    if !matches!(crate::package::state(&folder), crate::package::State::Ready(_)) {
        eprintln!("polynya: the package is not installed, see the log for details");
        return 1;
    }
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{FindWindowW, PostMessageW},
    };
    // SAFETY: both strings are static and null-terminated.
    if let Ok(hwnd) = unsafe { FindWindowW(windows::core::w!("Polynya"), windows::core::w!("Polynya")) } {
        // SAFETY: posting a message to a window handle has no memory-safety preconditions.
        let _ = unsafe { PostMessageW(Some(hwnd), crate::WM_APP_PACKAGE, WPARAM(0), LPARAM(0)) };
    }
    println!("polynya: Windows notifications are ready");
    0
}

/// The Polynya mark in characters (`tools/ascii.py`), the same as in "About this PC".
const ART: &str = include_str!("../assets/logo/mark-ascii.json");

/// The mark for the console; `color` colors each character with 24-bit ANSI codes.
fn logo(color: bool) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(ART) else { return String::new() };
    let lines = v["lines"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line = line.as_str().unwrap_or("");
        if !color {
            out.push(line.to_string());
            continue;
        }
        let mut s = String::new();
        for (j, ch) in line.chars().enumerate() {
            match v["colors"][i][j].as_str().and_then(proto::parse_hex) {
                Some([r, g, b]) if ch != ' ' => s += &format!("\x1b[38;2;{r};{g};{b}m{ch}"),
                _ => s.push(ch),
            }
        }
        out.push(s + "\x1b[0m");
    }
    out.join("\n")
}

/// Whether output is a console that understands colors; enables ANSI codes in it. To a file or
/// another program the mark goes without codes.
fn color_console() -> bool {
    // SAFETY: `out` is the process's standard output handle; `mode` outlives the call.
    unsafe {
        let Ok(out) = GetStdHandle(STD_OUTPUT_HANDLE) else { return false };
        let mut mode = CONSOLE_MODE::default();
        GetConsoleMode(out, &mut mode).is_ok() && SetConsoleMode(out, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING).is_ok()
    }
}

fn clock() -> String {
    // SAFETY: `GetLocalTime` has no preconditions.
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

fn clip(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() > MAX_BODY { format!("{}…", line.chars().take(MAX_BODY).collect::<String>()) } else { line.to_string() }
}

fn push(args: &[String]) -> Result<Msg, String> {
    let mut positional = Vec::new();
    let (mut key, mut ttl, mut icon, mut meta) = (None, DEFAULT_TTL, "info".to_string(), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{a}: missing value"));
        match a.as_str() {
            "--key" => key = Some(value()?),
            "--ttl" => ttl = value()?.parse().map_err(|_| "--ttl: expected a number of seconds")?,
            "--icon" => icon = value()?,
            "--meta" => meta = Some(value()?),
            _ => positional.push(a.clone()),
        }
    }
    let mut positional = positional.into_iter();
    let title = positional.next().ok_or("a title is required")?;
    // Without a key every notification is separate: unique by send time.
    let key = key.unwrap_or_else(|| format!("push:{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    Ok(Msg::Chip {
        key,
        source: None,
        icon,
        title,
        body: clip(&positional.next().unwrap_or_default()),
        meta: meta.unwrap_or_else(|| format!("polynya push · {}", clock())),
        ttl_ms: (ttl > 0).then_some(ttl * 1000),
        animated: false,
        app: None,
        popup: false,
        swatches: Vec::new(),
        mark: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_args() {
        let a: Vec<String> = ["build done", "pnpm build · 12.4 s", "--icon", "ok", "--ttl", "0", "--key", "build"].map(String::from).to_vec();
        let Ok(Msg::Chip { key, icon, title, body, ttl_ms, .. }) = push(&a) else { panic!() };
        assert_eq!((key.as_str(), icon.as_str(), title.as_str(), body.as_str(), ttl_ms), ("build", "ok", "build done", "pnpm build · 12.4 s", None));
        assert!(push(&[]).is_err());
    }

    #[test]
    fn logo_without_color_is_plain() {
        let plain = logo(false);
        assert!(!plain.is_empty() && !plain.contains('\x1b'));
        assert!(logo(true).contains("\x1b[38;2;"));
    }

    #[test]
    fn clip_takes_first_line() {
        assert_eq!(clip("\n  first  \nsecond"), "first");
        assert!(clip(&"а".repeat(300)).ends_with('…'));
    }
}
