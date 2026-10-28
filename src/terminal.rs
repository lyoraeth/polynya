// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Long terminal commands: a notification when a command finishes while the terminal is out of view.
//!
//! No user setup and no polling. The command's progress does not matter, the moment of leaving does:
//! when the foreground switches from the terminal to another window, the island looks once at the
//! terminal's process tree, finds the shells the terminal itself launched and the commands running
//! in them, and waits for them to exit (the system reports process exit). When a shell frees up, the
//! whole chain like `build && test` is done, and a notification arrives. Returning to the terminal
//! earlier drops the watch.
//!
//! System console events (`EVENT_CONSOLE_START_APPLICATION`) do not work here: Windows Terminal and
//! VS Code run through ConPTY, which does not send them.
//!
//! Shells not launched by the terminal itself (the bash an AI agent in the editor runs commands
//! through) are ignored, otherwise every agent build would be a notification.

use std::{cell::RefCell, collections::HashMap, ffi::c_void, sync::OnceLock, time::Duration};

use windows::{
    Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS},
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, UNICODE_STRING, WAIT_OBJECT_0},
        System::{
            SystemInformation::GetSystemTimeAsFileTime,
            Threading::{
                GetExitCodeProcess, GetProcessTimes, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
                RegisterWaitForSingleObject, UnregisterWaitEx, WT_EXECUTEONLYONCE, WaitForSingleObject,
            },
        },
    },
};

use crate::{apps, i18n, icons::Icon, island::zone::Note};

/// Shells: their direct children are the user's commands.
const SHELLS: [&str; 8] = ["pwsh.exe", "powershell.exe", "cmd.exe", "bash.exe", "sh.exe", "zsh.exe", "fish.exe", "nu.exe"];
/// Console hosts among a shell's children are plumbing, not commands.
const HOSTS: [&str; 2] = ["conhost.exe", "openconsole.exe"];
/// How deep below the terminal window's process to look for its shells.
const DEPTH: usize = 4;
/// `ProcessCommandLineInformation`, not named in `windows`.
const PROCESS_COMMAND_LINE: PROCESSINFOCLASS = PROCESSINFOCLASS(60);
/// Longer details are truncated.
const MAX_BODY: usize = 300;
const TTL: Duration = Duration::from_secs(10);

/// A command whose exit is awaited.
struct Child {
    handle: HANDLE,
    wait: HANDLE,
}

/// A shell running a command while the terminal is out of view.
struct Watch {
    /// The terminal window's process: returning to it drops the watch.
    terminal: u32,
    /// Start of the chain's first command (FILETIME, 100 ns).
    first: u64,
    /// Command line of the first command; the chip is named after it.
    cmdline: String,
    /// Exit code of the last finished command: in `a && b` the failed one is last.
    code: u32,
    children: HashMap<u32, Child>,
}

impl Watch {
    fn close(&mut self) {
        for (_, c) in self.children.drain() {
            release(c);
        }
    }
}

fn release(c: Child) {
    // SAFETY: `c` owns both handles, registered by `add_child`; each is released exactly once here.
    unsafe {
        // Do not wait for the callback: it only wakes the UI thread.
        let _ = UnregisterWaitEx(c.wait, None);
        let _ = CloseHandle(c.handle);
    }
}

thread_local! {
    /// Watches by shell pid; live on the UI thread.
    static WATCHES: RefCell<HashMap<u32, Watch>> = RefCell::new(HashMap::new());
}

/// Wakes the UI thread when a command exits; called from the system thread pool.
static WAKE: OnceLock<fn()> = OnceLock::new();

pub fn init(wake: fn()) {
    let _ = WAKE.set(wake);
}

unsafe extern "system" fn on_exit(_: *mut c_void, _: bool) {
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

fn open(pid: u32) -> Option<HANDLE> {
    // SAFETY: `OpenProcess` has no memory-safety preconditions; failure is returned.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid).ok() }
}

fn command_line(h: HANDLE) -> String {
    let mut buf = vec![0u8; 4096];
    let mut len = 0u32;
    // SAFETY: `h` is an open process handle; `buf` is writable for the length passed. On success the
    // buffer starts with a `UNICODE_STRING` whose `Buffer` points into `buf` for `Length` bytes.
    unsafe {
        let mut st = NtQueryInformationProcess(h, PROCESS_COMMAND_LINE, buf.as_mut_ptr() as _, buf.len() as u32, &mut len);
        if st.is_err() && len as usize > buf.len() {
            buf.resize(len as usize, 0);
            st = NtQueryInformationProcess(h, PROCESS_COMMAND_LINE, buf.as_mut_ptr() as _, buf.len() as u32, &mut len);
        }
        if st.is_err() {
            return String::new();
        }
        let us = &*(buf.as_ptr() as *const UNICODE_STRING);
        if us.Buffer.is_null() {
            return String::new();
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(us.Buffer.0, us.Length as usize / 2))
    }
}

fn filetime(f: FILETIME) -> u64 {
    (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64
}

fn created(h: HANDLE) -> u64 {
    let (mut c, mut e, mut k, mut u) = Default::default();
    // SAFETY: `h` is an open process handle and all four out pointers are valid.
    match unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) } {
        Ok(()) => filetime(c),
        Err(_) => now(),
    }
}

fn now() -> u64 {
    // SAFETY: `GetSystemTimeAsFileTime` has no preconditions.
    filetime(unsafe { GetSystemTimeAsFileTime() })
}

fn exe_of(procs: &[apps::Proc], pid: u32) -> &str {
    procs.iter().find(|p| p.pid == pid).map_or("", |p| p.exe.as_str())
}

/// Shells the terminal `terminal` launched itself (their parent has the same exe as the window:
/// Windows Terminal, or the VS Code helper process that hosts terminals). A classic console window
/// belongs to the shell itself.
fn terminal_shells(procs: &[apps::Proc], terminal: u32) -> Vec<u32> {
    let term_exe = exe_of(procs, terminal);
    if SHELLS.contains(&term_exe) {
        return vec![terminal];
    }
    let under_terminal = |mut pid: u32| {
        for _ in 0..DEPTH {
            if pid == terminal {
                return true;
            }
            match procs.iter().find(|p| p.pid == pid) {
                Some(p) if p.parent != pid => pid = p.parent,
                _ => return false,
            }
        }
        false
    };
    procs.iter().filter(|p| SHELLS.contains(&p.exe.as_str()) && exe_of(procs, p.parent) == term_exe && under_terminal(p.parent)).map(|p| p.pid).collect()
}

/// Commands running in a shell: its direct children, except console hosts.
fn commands(procs: &[apps::Proc], shell: u32) -> Vec<u32> {
    procs.iter().filter(|p| p.parent == shell && !HOSTS.contains(&p.exe.as_str())).map(|p| p.pid).collect()
}

/// Starts waiting for command `pid` in a shell to exit.
fn add_child(w: &mut Watch, pid: u32) {
    if w.children.contains_key(&pid) {
        return;
    }
    let Some(handle) = open(pid) else { return };
    let mut wait = HANDLE::default();
    // SAFETY: `handle` is an open process handle with SYNCHRONIZE access, kept open until the wait is
    // unregistered; `on_exit` is a plain function that touches no context.
    let ok = unsafe { RegisterWaitForSingleObject(&mut wait, handle, Some(on_exit), None, INFINITE, WT_EXECUTEONLYONCE) }.is_ok();
    if !ok {
        // SAFETY: `handle` was opened above and is closed once on this path.
        let _ = unsafe { CloseHandle(handle) };
        return;
    }
    let start = created(handle);
    if start < w.first {
        w.first = start;
        w.cmdline = command_line(handle);
    }
    w.children.insert(pid, Child { handle, wait });
}

/// The foreground window changed: `from` is the previous one's process, `to` the new one's.
pub fn foreground_changed(from: u32, to: u32) {
    // Back in the terminal: it is in view anyway.
    WATCHES.with(|m| {
        m.borrow_mut().retain(|_, w| {
            let keep = w.terminal != to;
            if !keep {
                w.close();
            }
            keep
        })
    });
    if from == 0 || from == to {
        return;
    }
    let procs = apps::processes();
    for shell in terminal_shells(&procs, from) {
        let cmds = commands(&procs, shell);
        if cmds.is_empty() || WATCHES.with(|m| m.borrow().contains_key(&shell)) {
            continue;
        }
        let mut w = Watch { terminal: from, first: u64::MAX, cmdline: String::new(), code: 0, children: HashMap::new() };
        for pid in cmds {
            add_child(&mut w, pid);
        }
        if !w.children.is_empty() {
            WATCHES.with(|m| m.borrow_mut().insert(shell, w));
        }
    }
}

/// Processes finished commands. Returns notifications for freed shells whose chain ran at least
/// `min` while the terminal is still out of view (`front` is the foreground window's process).
///
/// Call with a short delay after waking: the chain's next command does not start instantly after
/// the previous one.
pub fn settle(min: Duration, front: u32) -> Vec<Note> {
    let mut out = Vec::new();
    let mut procs: Option<Vec<apps::Proc>> = None;
    WATCHES.with(|m| {
        m.borrow_mut().retain(|&shell, w| {
            // SAFETY: `c.handle` is an open process handle owned by the watch.
            let done: Vec<u32> = w.children.iter().filter(|(_, c)| unsafe { WaitForSingleObject(c.handle, 0) } == WAIT_OBJECT_0).map(|(p, _)| *p).collect();
            for pid in done {
                if let Some(c) = w.children.remove(&pid) {
                    // SAFETY: `c.handle` is still open here, and `w.code` is a valid out pointer.
                    let _ = unsafe { GetExitCodeProcess(c.handle, &mut w.code) };
                    release(c);
                }
            }
            if !w.children.is_empty() {
                return true;
            }
            // The chain's next command may have started already.
            let procs = procs.get_or_insert_with(apps::processes);
            for pid in commands(procs, shell) {
                add_child(w, pid);
            }
            if !w.children.is_empty() {
                return true;
            }
            let took = Duration::from_nanos(now().saturating_sub(w.first) * 100);
            if took >= min && front != w.terminal {
                out.push(note(shell, &w.cmdline, took, w.code as i32));
            }
            false
        })
    });
    out
}

/// Command line arguments, honoring quotes.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Program name without path and extension.
fn stem(arg: &str) -> String {
    let file = arg.rsplit(['\\', '/']).next().unwrap_or(arg);
    match file.rsplit_once('.') {
        Some((name, ext)) if ["exe", "cmd", "bat", "cjs", "js", "mjs", "ps1", "py"].contains(&ext.to_lowercase().as_str()) => name.to_string(),
        _ => file.to_string(),
    }
}

/// Short command name for the chip, as a person typed it: `cargo build`, `pnpm build`.
///
/// Wrappers are stripped: `node …\pnpm.cjs build` is `pnpm build`, `cmd /c "vite build"` is
/// `vite build`.
fn short_command(cmdline: &str) -> String {
    let mut args = split_args(cmdline);
    loop {
        let Some(first) = args.first() else { return String::new() };
        let name = stem(first).to_lowercase();
        let wrapper = matches!(name.as_str(), "node" | "python" | "python3" | "py" | "cmd" | "pwsh" | "powershell");
        if !wrapper {
            break;
        }
        // The wrapper and its switches (`/c`, `-File`, `-u`) are dropped.
        let rest: Vec<String> = args.into_iter().skip(1).skip_while(|a| a.starts_with('/') || a.starts_with('-')).collect();
        if rest.is_empty() {
            return name;
        }
        // `cmd /c "pnpm.cmd build"`: the whole command in one argument.
        args = if rest[0].contains(' ') { split_args(&rest[0]).into_iter().chain(rest.into_iter().skip(1)).collect() } else { rest };
    }
    let mut parts = vec![stem(&args[0])];
    parts.extend(args.iter().skip(1).take_while(|a| !a.starts_with('-')).take(2).cloned());
    parts.join(" ")
}

fn duration(d: Duration, tr: &i18n::Strings) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s} {}", tr.unit_s),
        60..3600 => format!("{} {} {} {}", s / 60, tr.unit_min, s % 60, tr.unit_s),
        _ => format!("{} {} {} {}", s / 3600, tr.unit_h, s % 3600 / 60, tr.unit_min),
    }
}

fn note(pid: u32, cmdline: &str, took: Duration, code: i32) -> Note {
    let tr = i18n::t();
    // SAFETY: `GetLocalTime` has no preconditions.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let body: String = cmdline.trim().chars().take(MAX_BODY).collect();
    Note {
        key: format!("term:{pid}"),
        source: None,
        app: None,
        popup: false,
        icon: if code == 0 { Icon::Ok } else { Icon::Fail },
        title: format!("{} · {}", short_command(cmdline), duration(took, tr)),
        body,
        meta: format!("{} · {} {code} · {:02}:{:02}", tr.terminal, tr.exit_code, t.wHour, t.wMinute),
        ttl: Some(TTL),
        animated: false,
        swatches: Vec::new(),
        mark: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_commands() {
        assert_eq!(short_command(r#""C:\Users\Danil\.cargo\bin\cargo.exe" build --release"#), "cargo build");
        assert_eq!(short_command(r#""C:\nvm4w\nodejs\node.exe" "C:\nvm4w\nodejs\node_modules\pnpm\bin\pnpm.cjs" build"#), "pnpm build");
        assert_eq!(short_command(r#"C:\WINDOWS\system32\cmd.exe /d /s /c "pnpm.cmd run dev""#), "pnpm run dev");
        assert_eq!(short_command("python -u train.py --epochs 3"), "train");
        assert_eq!(short_command("git push origin master"), "git push origin");
    }

    #[test]
    fn durations() {
        // The language is passed in: the current one is set once per process and another test may set it first.
        let en = i18n::table(i18n::Lang::En);
        assert_eq!(duration(Duration::from_secs(48), en), "48 s");
        assert_eq!(duration(Duration::from_secs(125), en), "2 min 5 s");
        assert_eq!(duration(Duration::from_secs(3780), en), "1 h 3 min");
    }
}
