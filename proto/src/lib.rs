// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: MIT

//! Protocol definitions and IPC utilities between the Polynya core and its modules.
//!
//! Modules communicate with the core exclusively via these messages, ensuring consistent
//! behavior regardless of whether a module is built-in or external. The transport layer
//! (currently session-specific named pipes) can be changed without modifying module logic.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_PIPE_BUSY, GENERIC_WRITE},
        Globalization::GetUserDefaultUILanguage,
        Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING, WriteFile},
        System::{
            Pipes::WaitNamedPipeW,
            RemoteDesktop::ProcessIdToSessionId,
            SystemInformation::GetLocalTime,
            Threading::{GetCurrentProcessId, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE},
        },
    },
    core::{HSTRING, PCWSTR, w},
};

/// Returns the session-specific named pipe path to prevent cross-session message leakage.
pub fn pipe() -> HSTRING {
    let mut session = 0u32;
    // SAFETY: `session` is a valid out pointer for the call.
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
    }
    HSTRING::from(format!(r"\\.\pipe\Polynya-{session}"))
}

/// Mutex name to detect if the core process is currently running.
pub const INSTANCE_MUTEX: PCWSTR = w!("Polynya.single-instance");
/// Maximum message size to prevent memory exhaustion from malformed pipe data.
pub const MAX_MSG: usize = 64 * 1024;

/// Message sent to the core.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
// Messages are parsed and handled one at a time, so the size of the largest variant costs nothing.
#[allow(clippy::large_enum_variant)]
pub enum Msg {
    /// Display or update a chip in the right zone.
    Chip {
        key: String,
        /// Groups chips from the same source to prevent queue blocking (e.g., chat messages).
        #[serde(default)]
        source: Option<String>,
        /// Built-in icon name (`ok`, `fail`, `info`, `key`, `claude`, `agent`) or
        /// a color circle: `color:#rrggbb`. Defaults to `info`.
        #[serde(default = "default_icon")]
        icon: String,
        /// Short text displayed in the chip.
        title: String,
        /// Detailed text shown in the marquee on hover and in the expanded view.
        #[serde(default)]
        body: String,
        /// Source and timestamp, displayed small in the expanded view.
        #[serde(default)]
        meta: String,
        /// Lifetime in ms. `None` means it persists until closed or replaced.
        #[serde(default)]
        ttl_ms: Option<u64>,
        /// Indicates the icon is animated (work in progress).
        #[serde(default)]
        animated: bool,
        /// Windows notification AUMID. Clicking opens the source application.
        #[serde(default)]
        app: Option<String>,
        /// Immediately expand as a compact popup (replaces system banner).
        #[serde(default)]
        popup: bool,
        /// Array of `#rrggbb` colors in the expanded chip. Clicking copies the color.
        #[serde(default)]
        swatches: Vec<String>,
        /// Index of the highlighted color in the swatches array.
        #[serde(default)]
        mark: Option<usize>,
    },
    /// Dismiss a chip by its key.
    Dismiss { key: String },
}

fn default_icon() -> String {
    "info".into()
}

/// Module dropdown screen: optional title with a "back" button and a list of rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Screen {
    #[serde(default)]
    pub title: Option<String>,
    pub rows: Vec<Row>,
}

/// A single row in a screen. The core renders these in the island style and handles arrow navigation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Row {
    /// Selectable item. `hint` is right-aligned; `keys` is a shortcut in its own narrow column
    /// (`Ctrl+Alt+D`), for keys that are not the module's own hotkeys (those come with `command`). If `confirm` is present, the first press shows the confirmation text and
    /// a shrinking bar; the second press executes it.
    Item {
        id: String,
        text: String,
        #[serde(default)]
        hint: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        keys: String,
        /// The function this row runs (`[[functions]]` id). The core shows its hotkey in `keys`,
        /// and choosing the row sends `invoke` with `source: menu` instead of `select`.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        command: String,
        #[serde(default)]
        confirm: Option<String>,
    },
    /// Toggle switch: label on the left, slider on the right. Pressing acts like an Item.
    Toggle { id: String, text: String, on: bool },
    /// Input field: pressing starts inline editing. Enter submits (`ModIn::Submit`), Esc cancels.
    /// Shows `placeholder` when empty.
    Input {
        id: String,
        text: String,
        #[serde(default)]
        placeholder: String,
    },
    /// Full-width button, centered text. Standard has a background (primary action);
    /// `plain` has no background, highlighting only on selection (secondary action).
    Button {
        id: String,
        text: String,
        #[serde(default)]
        plain: bool,
    },
    /// Muted text paragraph. Lines split by `\n`, no word wrapping.
    Text { text: String },
    /// Horizontal divider.
    Divider,
    /// Section subtitle: small muted text above a group of rows. Empty title acts as a spacer.
    Section { title: String },
    /// Large title with a subtitle below it.
    Header { title: String, subtitle: String },
    /// Left-aligned label, right-aligned value.
    Pair { label: String, value: String },
    /// Monospace ASCII art, centered. Each character can have a specific `#rrggbb` color
    /// (`colors[row][char]`). Empty string defaults to the text color.
    Art {
        lines: Vec<String>,
        #[serde(default)]
        colors: Vec<Vec<String>>,
    },
}

/// Message from a module to the core: one JSON line on the module's stdout.
///
/// A module may also write [`Msg`] lines (`chip`, `dismiss`) to its stdout instead of
/// using the pipe.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum ModOut {
    /// Opens the module's dropdown with this screen, replacing whatever the dropdown showed.
    /// Without `at` it opens under the search block (as a page of search if the user just
    /// chose the module's command there); `at` is a span of screen pixels `[left, right]` on
    /// the taskbar to open under instead.
    Open {
        screen: Screen,
        #[serde(default)]
        at: Option<[i32; 2]>,
    },
    /// Shows the next screen in the open dropdown, with a "back" button to the current one.
    Push { screen: Screen },
    /// Replaces the current screen in place, e.g. after a toggle changed.
    Update { screen: Screen },
    /// Closes the module's dropdown. Right after an `invoke`, also closes the launcher.
    Close,
    /// Puts text on the clipboard.
    Copy { text: String },
}

/// Message from the core to a module: one JSON line on the module's stdin.
/// EOF on stdin means the core has exited or stopped the module, and the module should exit.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum ModIn {
    /// Run a command: `main` opens the module's main interface, anything else is one of its
    /// `[[functions]]`. The same function arrives the same way from every place it can be run;
    /// `source` says which, and the module decides what to start from: the selection, the
    /// clipboard or an empty field.
    Invoke {
        command: String,
        source: Source,
        /// The text in the search line when run from search.
        #[serde(default)]
        query: String,
        /// The selection in the active app: only for a `selection = true` function run by its
        /// hotkey, and empty if nothing was selected.
        #[serde(default)]
        selection: String,
        /// The clipboard text, always.
        #[serde(default)]
        clipboard: String,
    },
    /// A screen item, toggle or button was selected.
    Select { id: String },
    /// Text was submitted in a `Row::Input` field.
    Submit { id: String, text: String },
    /// The module dropdown was closed (Esc, click outside, or another panel opened).
    Closed,
}

/// Where a command was run from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A row of the module's own screen with `command`.
    Menu,
    /// A search result.
    Search,
    /// The command's global hotkey.
    Hotkey,
}

/// A line a module wrote to its stdout: a dropdown message or a chip message.
#[derive(Debug, PartialEq)]
pub enum Line {
    Out(ModOut),
    Msg(Msg),
}

/// Parses one stdout line of a module. The error names what is wrong, for the log.
pub fn parse_line(line: &str) -> Result<Line, String> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("not JSON: {e}"))?;
    let op = v.get("op").and_then(|o| o.as_str()).ok_or("no \"op\" field")?.to_string();
    match op.as_str() {
        "chip" | "dismiss" => serde_json::from_value(v).map(Line::Msg),
        "open" | "push" | "update" | "close" | "copy" => serde_json::from_value(v).map(Line::Out),
        _ => return Err(format!("unknown op \"{op}\"")),
    }
    .map_err(|e| format!("op \"{op}\": {e}"))
}

/// Checks if the core process is currently running.
pub fn core_running() -> bool {
    // SAFETY: `INSTANCE_MUTEX` is a static null-terminated wide string.
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, INSTANCE_MUTEX) } {
        Ok(h) => {
            // SAFETY: `h` was just opened and is closed exactly once.
            let _ = unsafe { CloseHandle(h) };
            true
        }
        Err(_) => false,
    }
}

/// Sends a message to the running core.
///
/// Retries briefly if the pipe is busy (core is processing the previous message).
/// Fails immediately if the core is not running, preventing module hangs.
pub fn send(msg: &Msg) -> Result<(), String> {
    let data = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    for _ in 0..20 {
        // SAFETY: the pipe name HSTRING outlives the call; other pointers are null.
        let opened = unsafe { CreateFileW(&pipe(), GENERIC_WRITE.0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) };
        match opened {
            Ok(h) => {
                // SAFETY: `h` is the pipe just opened, and `data` outlives the synchronous write.
                let res = unsafe { WriteFile(h, Some(&data), None, None) };
                // SAFETY: `h` is closed exactly once.
                let _ = unsafe { CloseHandle(h) };
                return res.map_err(|e| e.to_string());
            }
            // SAFETY: the pipe name HSTRING outlives the call.
            Err(e) if e.code() == ERROR_PIPE_BUSY.to_hresult() => unsafe {
                let _ = WaitNamedPipeW(&pipe(), 500);
            },
            Err(_) if core_running() => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("Polynya is not running ({e})")),
        }
    }
    Err("the Polynya pipe is busy".into())
}

/// Parses a `#rrggbb` hex string into an RGB array.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Formats an RGB array into a `#rrggbb` hex string.
pub fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Log severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Written only if `log = debug` is set in config (verbose details, user content).
    Debug,
    Info,
    Warn,
    Error,
}

/// Maximum log file size before rotation to `polynya.old.log`.
const LOG_MAX: u64 = 1 << 20;

/// Appends a log entry to `polynya.log` in [`log_dir`].
/// Rotates to `polynya.old.log` if size exceeds `LOG_MAX`.
/// Uses append mode to ensure atomic line writes across concurrent threads/processes.
pub fn log(level: Level, area: &str, msg: &str) {
    use std::io::Write;
    static DEBUG: OnceLock<bool> = OnceLock::new();
    let debug = *DEBUG.get_or_init(|| config_value("log").as_deref() == Some("debug"));
    if level == Level::Debug && !debug {
        return;
    }
    let Some(dir) = log_dir() else { return };
    let path = dir.join("polynya.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX) {
        // Another process might have rotated it first; ignore rename errors.
        let _ = std::fs::rename(&path, dir.join("polynya.old.log"));
    }
    let _ = std::fs::create_dir_all(&dir);
    // SAFETY: `GetLocalTime` has no preconditions.
    let t = unsafe { GetLocalTime() };
    let tag = match level {
        Level::Debug => "DEBUG",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {tag:<5} {area:<8} {msg}\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[macro_export]
macro_rules! debug {
    ($area:expr, $($arg:tt)*) => { $crate::log($crate::Level::Debug, $area, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! info {
    ($area:expr, $($arg:tt)*) => { $crate::log($crate::Level::Info, $area, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($area:expr, $($arg:tt)*) => { $crate::log($crate::Level::Warn, $area, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($area:expr, $($arg:tt)*) => { $crate::log($crate::Level::Error, $area, &format!($($arg)*)) };
}

/// Retrieves a configuration value from `config.ini`.
fn config_value(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(data_dir().join("config.ini")).ok()?;
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().to_lowercase())
}

/// Name of the file that marks a portable copy when it lies next to the core exe.
pub const PORTABLE_MARKER: &str = "portable";

/// Root folder of a portable copy, `None` for an installed one.
///
/// A portable copy keeps everything next to its exe instead of the user profile, so it can live on
/// a flash drive or be deleted with its folder. The core exe sees the marker beside itself; a
/// module exe lives in `<root>\modules\<id>\` and looks two levels up. A module needs no hint from
/// the core this way: the packaged notifications module is started by Windows, not by the core.
pub fn portable_root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| portable_root_of(std::env::current_exe().ok()?.parent()?)).as_deref()
}

/// [`portable_root`] for an exe in folder `dir`.
fn portable_root_of(dir: &Path) -> Option<PathBuf> {
    let module_root = dir.parent().filter(|m| m.file_name().is_some_and(|n| n.eq_ignore_ascii_case("modules"))).and_then(Path::parent);
    [Some(dir), module_root].into_iter().flatten().find(|d| d.join(PORTABLE_MARKER).is_file()).map(Path::to_path_buf)
}

/// Configuration and data folder: `%APPDATA%\Polynya`, in a portable copy `<root>\data`.
pub fn data_dir() -> PathBuf {
    if let Some(root) = portable_root() {
        return root.join("data");
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Polynya")
}

/// Log folder: `%LOCALAPPDATA%\Polynya\logs`, in a portable copy `<root>\logs`.
pub fn log_dir() -> Option<PathBuf> {
    if let Some(root) = portable_root() {
        return Some(root.join("logs"));
    }
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"Polynya\logs"))
}

/// Returns the UI language code (`ru`, `en`, `zh`, `ja`).
/// Falls back to the Windows default UI language if not specified in config.
pub fn language() -> &'static str {
    // A module gets the core's language in `POLYNYA_LANG`; config.ini is for the core itself and
    // for modules started some other way.
    match std::env::var("POLYNYA_LANG").ok().or_else(|| config_value("language")).as_deref() {
        Some("ru") => "ru",
        Some("en") => "en",
        Some("zh") => "zh",
        Some("ja") => "ja",
        // The low 10 bits of a LANGID are the primary language.
        // SAFETY: `GetUserDefaultUILanguage` has no preconditions.
        _ => match unsafe { GetUserDefaultUILanguage() } & 0x3ff {
            0x19 => "ru",
            0x04 => "zh",
            0x11 => "ja",
            _ => "en",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_root_from_core_and_module() {
        let root = std::env::temp_dir().join(format!("polynya-portable-{}", std::process::id()));
        let module = root.join("modules").join("picker");
        std::fs::create_dir_all(&module).unwrap();
        assert_eq!(portable_root_of(&root), None);
        assert_eq!(portable_root_of(&module), None);
        std::fs::write(root.join(PORTABLE_MARKER), "").unwrap();
        assert_eq!(portable_root_of(&root).as_deref(), Some(root.as_path()));
        assert_eq!(portable_root_of(&module).as_deref(), Some(root.as_path()));
        // Two levels up counts only from a module folder.
        let other = root.join("tools").join("x");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(portable_root_of(&other), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn screen_roundtrip() {
        let m = ModOut::Open {
            screen: Screen {
                title: None,
                rows: vec![
                    Row::Item { id: "a".into(), text: "A".into(), hint: String::new(), keys: String::new(), command: String::new(), confirm: None },
                    Row::Divider,
                ],
            },
            at: Some([0, 43]),
        };
        let line = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<ModOut>(&line).unwrap(), m);
        assert!(line.contains(r#""type":"divider""#));
    }

    #[test]
    fn stdout_lines() {
        assert!(matches!(parse_line(r#"{"op":"close"}"#), Ok(Line::Out(ModOut::Close))));
        assert!(matches!(parse_line(r#"{"op":"chip","key":"k","title":"t"}"#), Ok(Line::Msg(_))));
        assert!(matches!(parse_line(r#"{"op":"update","screen":{"rows":[]}}"#), Ok(Line::Out(ModOut::Update { .. }))));
        assert_eq!(parse_line(r#"{"op":"show"}"#), Err("unknown op \"show\"".into()));
        assert!(parse_line("hello").unwrap_err().starts_with("not JSON"));
        assert!(parse_line(r#"{"op":"push"}"#).unwrap_err().contains("screen"));
    }

    #[test]
    fn minimal_chip_parses() {
        let m: Msg = serde_json::from_str(r#"{"op":"chip","key":"k","title":"t"}"#).unwrap();
        let Msg::Chip { icon, body, ttl_ms, animated, .. } = m else { panic!() };
        assert_eq!((icon.as_str(), body.as_str(), ttl_ms, animated), ("info", "", None, false));
    }

    #[test]
    fn old_chip_with_module_still_parses() {
        // The `module` field was removed; ensure backward compatibility with old senders.
        assert!(serde_json::from_str::<Msg>(r#"{"op":"chip","key":"k","title":"t","module":"x"}"#).is_ok());
    }
}
