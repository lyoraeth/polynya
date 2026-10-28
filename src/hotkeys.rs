// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Global hotkeys of module commands, and grabbing the user's selection for them.
//!
//! A command can declare `hotkey = "Ctrl+Alt+D"` in its manifest (the user can override or turn it
//! off in `config.ini`). With `selection = true` the core copies whatever is selected in the
//! foreground app before invoking the module: it sends Ctrl+C itself and waits for the clipboard to
//! change. With nothing selected the selection is empty; the module also gets the clipboard text
//! and decides what to work on.

use std::{
    cell::RefCell,
    sync::Mutex,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, HOT_KEY_MODIFIERS, INPUT, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, SendInput, UnregisterHotKey,
            VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RMENU, VK_RSHIFT, VK_RWIN,
        },
        WindowsAndMessaging::PostMessageW,
    },
};

/// Hotkey ids of module commands start here; lower ids belong to the core.
pub const FIRST_ID: i32 = 100;
/// How long to wait for the foreground app to put its selection on the clipboard.
const COPY_WAIT: Duration = Duration::from_millis(400);
/// An unassigned key sent before releasing Alt or Win, so the app does not open its menu and
/// Windows does not open Start.
const MASK_VK: u16 = 0xE8;

/// A registered hotkey of a module command.
#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    pub module: String,
    pub command: String,
    pub selection: bool,
}

thread_local! {
    /// Registered hotkeys by id, with their display text; live on the UI thread that owns the window.
    static BOUND: RefCell<Vec<(i32, Bound, String)>> = const { RefCell::new(Vec::new()) };
    /// Every hotkey the user set, registered or not, as shown: (module, command, keys).
    static SHOWN: RefCell<Vec<(String, String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Parses a hotkey like `Ctrl+Alt+D`, `Ctrl+Shift+F2` or `Win+Alt+Space`. Case-insensitive.
pub fn parse(s: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for part in s.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" => mods |= MOD_WIN,
            _ if key.is_some() => return None,
            k => key = Some(vk(k)?),
        }
    }
    // A hotkey without a modifier would swallow a plain key everywhere.
    (mods.0 != 0).then_some((mods, key?))
}

/// Virtual key code of a key name: a letter, a digit, F1–F24 or a few named keys.
fn vk(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(c.to_ascii_uppercase() as u32);
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=24).contains(n)) {
        return Some(0x70 + n - 1);
    }
    Some(match name {
        "space" => 0x20,
        "enter" => 0x0D,
        "tab" => 0x09,
        "insert" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "`" | "backquote" => 0xC0,
        "-" | "minus" => 0xBD,
        "=" | "equal" => 0xBB,
        "," | "comma" => 0xBC,
        "." | "period" => 0xBE,
        "/" | "slash" => 0xBF,
        ";" | "semicolon" => 0xBA,
        _ => return None,
    })
}

/// Registers the hotkeys of module commands for `hwnd`, replacing the previous set. A hotkey that
/// is malformed or taken by another app is skipped and reported in the log.
pub fn sync(hwnd: HWND, wanted: Vec<(String, Bound)>) {
    let old = BOUND.with(|b| std::mem::take(&mut *b.borrow_mut()));
    for (id, ..) in old {
        // SAFETY: `hwnd` is our window; unregistering an id that is not registered only fails.
        let _ = unsafe { UnregisterHotKey(Some(hwnd), id) };
    }
    let mut bound = Vec::new();
    let shown = wanted.iter().filter(|(k, _)| parse(k).is_some()).map(|(k, b)| (b.module.clone(), b.command.clone(), display(k))).collect();
    SHOWN.with(|s| *s.borrow_mut() = shown);
    for (i, (keys, b)) in wanted.into_iter().enumerate() {
        let Some((mods, key)) = parse(&keys) else {
            proto::warn!("hotkeys", "{}/{}: cannot parse hotkey \"{keys}\"", b.module, b.command);
            continue;
        };
        let id = FIRST_ID + i as i32;
        // SAFETY: `hwnd` is our window on this thread; the id is unique within it.
        match unsafe { RegisterHotKey(Some(hwnd), id, mods | MOD_NOREPEAT, key) } {
            Ok(()) => bound.push((id, b, display(&keys))),
            Err(_) => proto::warn!("hotkeys", "{}/{}: {keys} is taken by another app", b.module, b.command),
        }
    }
    proto::info!("hotkeys", "{} module hotkeys registered", bound.len());
    BOUND.with(|b| *b.borrow_mut() = bound);
}

/// The module command bound to hotkey `id`.
pub fn lookup(id: i32) -> Option<Bound> {
    BOUND.with(|b| b.borrow().iter().find(|(i, ..)| *i == id).map(|(_, b, _)| b.clone()))
}

/// The hotkey of a module command as the user sees it (`Ctrl+Alt+D`).
pub fn keys(module: &str, command: &str) -> Option<String> {
    SHOWN.with(|s| s.borrow().iter().find(|(m, c, _)| m == module && c == command).map(|(.., k)| k.clone()))
}

/// Puts the hotkeys of `module`'s functions into its screen rows bound to them with `command`:
/// the module does not know them, the user may have changed them.
pub fn fill(module: &str, screen: &mut proto::Screen) {
    for row in &mut screen.rows {
        if let proto::Row::Item { keys, command, .. } = row
            && !command.is_empty()
            && keys.is_empty()
            && let Some(k) = self::keys(module, command)
        {
            *keys = k;
        }
    }
}

/// A hotkey written any way (`ctrl + alt+d`) in one spelling: modifiers in the Windows order,
/// then the key.
pub fn display(keys: &str) -> String {
    let parts: Vec<String> = keys.split('+').map(|p| p.trim().to_ascii_lowercase()).collect();
    let mut out: Vec<String> = ["win", "ctrl", "alt", "shift"]
        .iter()
        .filter(|m| parts.iter().any(|p| p == *m || (**m == "ctrl" && p == "control")))
        .map(|m| [m[..1].to_ascii_uppercase(), m[1..].to_string()].concat())
        .collect();
    if let Some(key) = parts.iter().find(|p| !matches!(p.as_str(), "win" | "ctrl" | "control" | "alt" | "shift")) {
        out.push(match key.as_str() {
            "pageup" => "PgUp".into(),
            "pagedown" => "PgDn".into(),
            "insert" => "Ins".into(),
            "backquote" => "`".into(),
            _ => {
                let mut c = key.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            }
        });
    }
    out.join("+")
}

/// The result of [`grab`]: the command and the text it works on.
static GRABBED: Mutex<Option<(Bound, String)>> = Mutex::new(None);

/// Copies the selection of the foreground app in a background thread, then posts `msg` to `hwnd`;
/// [`take`] returns the command and the text.
pub fn grab(b: Bound, hwnd: HWND, msg: u32) {
    let hwnd = hwnd.0 as usize;
    let _ = std::thread::Builder::new().name("selection".into()).spawn(move || {
        // Nothing selected is empty: the module has the clipboard in `invoke` and decides itself.
        let text = copy_selection().unwrap_or_default();
        if let Ok(mut g) = GRABBED.lock() {
            *g = Some((b, text));
        }
        // SAFETY: posting a message has no memory-safety preconditions; a stale window only fails.
        let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), msg, WPARAM(0), LPARAM(0)) };
    });
}

/// Takes the result of the last [`grab`].
pub fn take() -> Option<(Bound, String)> {
    GRABBED.lock().ok()?.take()
}

/// Sends Ctrl+C to the foreground app and returns the text it put on the clipboard; `None` if the
/// clipboard did not change (nothing selected, or the app does not copy on Ctrl+C). The user's
/// clipboard is put back afterwards: grabbing the selection must not cost them what they copied.
fn copy_selection() -> Option<String> {
    let saved = snapshot();
    let text = send_copy();
    if text.is_some() {
        restore(&saved);
    }
    text
}

/// Clipboard formats that are GDI handles, not memory: they cannot be copied as bytes (a picture
/// is also there as `CF_DIB`, which is memory).
const HANDLE_FORMATS: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];

/// The clipboard contents as `(format, bytes)` for every memory-backed format.
fn snapshot() -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    // SAFETY: the clipboard is opened and closed here; each handle is locked, read for its own
    // `GlobalSize` bytes and unlocked before the next one.
    unsafe {
        if OpenClipboard(None).is_err() {
            return out;
        }
        let mut f = EnumClipboardFormats(0);
        while f != 0 {
            if !HANDLE_FORMATS.contains(&f)
                && let Ok(h) = GetClipboardData(f)
            {
                let mem = windows::Win32::Foundation::HGLOBAL(h.0);
                let p = GlobalLock(mem) as *const u8;
                if !p.is_null() {
                    out.push((f, std::slice::from_raw_parts(p, GlobalSize(mem)).to_vec()));
                    let _ = GlobalUnlock(mem);
                }
            }
            f = EnumClipboardFormats(f);
        }
        let _ = CloseClipboard();
    }
    out
}

/// Puts a [`snapshot`] back on the clipboard.
fn restore(saved: &[(u32, Vec<u8>)]) {
    if saved.is_empty() {
        return;
    }
    // SAFETY: each buffer is allocated for exactly its bytes and handed to the system with
    // `SetClipboardData`, which takes ownership on success.
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        for (f, bytes) in saved {
            let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else { continue };
            let p = GlobalLock(mem) as *mut u8;
            if p.is_null() {
                continue;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(mem);
            let _ = SetClipboardData(*f, Some(windows::Win32::Foundation::HANDLE(mem.0)));
        }
        let _ = CloseClipboard();
    }
}

/// Sends Ctrl+C and waits for the clipboard to change.
fn send_copy() -> Option<String> {
    // SAFETY: plain Win32 calls; every input array is fully initialized and outlives `SendInput`.
    unsafe {
        let before = GetClipboardSequenceNumber();
        // The hotkey's own modifiers are still held: released first, or the app would see
        // Ctrl+Alt+C. The mask key keeps a released Alt from opening the app's menu.
        let held = |vk: VIRTUAL_KEY| GetAsyncKeyState(vk.0 as i32) < 0;
        let mut seq = vec![crate::key(VIRTUAL_KEY(MASK_VK), false), crate::key(VIRTUAL_KEY(MASK_VK), true)];
        seq.extend([VK_LMENU, VK_RMENU, VK_LSHIFT, VK_RSHIFT, VK_LWIN, VK_RWIN].into_iter().filter(|&k| held(k)).map(|k| crate::key(k, true)));
        seq.extend([crate::key(VK_CONTROL, false), crate::key(VK_C, false), crate::key(VK_C, true), crate::key(VK_CONTROL, true)]);
        SendInput(&seq, size_of::<INPUT>() as i32);
        let start = Instant::now();
        while start.elapsed() < COPY_WAIT {
            std::thread::sleep(Duration::from_millis(10));
            if GetClipboardSequenceNumber() != before {
                // The app may still be writing the formats; give it a moment.
                std::thread::sleep(Duration::from_millis(20));
                return crate::launcher::clipboard_text();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hotkeys() {
        assert_eq!(parse("Ctrl+Alt+D"), Some((MOD_CONTROL | MOD_ALT, 'D' as u32)));
        assert_eq!(parse("ctrl + shift + f2"), Some((MOD_CONTROL | MOD_SHIFT, 0x71)));
        assert_eq!(parse("Win+Space"), Some((MOD_WIN, 0x20)));
        assert_eq!(parse("D"), None);
        assert_eq!(parse("Ctrl+Alt"), None);
        assert_eq!(parse("Ctrl+D+E"), None);
        assert_eq!(parse("Ctrl+Nope"), None);
        assert_eq!(display("alt + ctrl+d"), "Ctrl+Alt+D");
        assert_eq!(display("shift+win+pageup"), "Win+Shift+PgUp");
    }
}
