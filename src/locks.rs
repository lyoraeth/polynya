// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Caps Lock and Num Lock: a right-zone notification on toggle.
//!
//! The keyboard is read via Raw Input with `RIDEV_INPUTSINK`, not a low-level hook: a hook holds
//! every key press system-wide until the hook thread handles it, while Raw Input just sends the
//! window a copy of the event and never delays input.
//!
//! Toggle state is tracked here from key presses. For an unfocused window `GetKeyState` lags:
//! the system updates it only when the thread processes keyboard messages, which go to the focused
//! window, and with fast presses the lag accumulates. So `GetKeyState` is read only as the initial
//! value when listening starts.

use std::{cell::Cell, time::Duration};

use windows::Win32::{
    Foundation::{HWND, LPARAM},
    UI::{
        Input::{
            GetRawInputData, HRAWINPUT,
            KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_NUMLOCK},
            RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
        },
        WindowsAndMessaging::RI_KEY_BREAK,
    },
};

use crate::{i18n, icons::Icon, island::zone::Note};

/// Lifetime of the toggle notification.
const TTL: Duration = Duration::from_secs(3);

/// Toggle: whether it is on and whether the key is held right now.
#[derive(Clone, Copy, Default)]
struct Toggle {
    on: bool,
    down: bool,
}

thread_local! {
    /// Caps Lock and Num Lock; live on the UI thread, which receives `WM_INPUT`.
    static STATE: Cell<[Toggle; 2]> = const { Cell::new([Toggle { on: false, down: false }; 2]) };
}

const KEYS: [(VIRTUAL_KEY, &str, &str); 2] = [(VK_CAPITAL, "caps", "Caps Lock"), (VK_NUMLOCK, "num", "Num Lock")];

/// Makes `hwnd` receive keyboard input even without focus; `None` turns it off.
///
/// On enable, toggle states are read from the system: while listening was off (the island hid
/// under a game), they may have been toggled.
pub fn listen(hwnd: Option<HWND>) {
    if hwnd.is_some() {
        // SAFETY: `GetKeyState` has no preconditions.
        STATE.with(|s| s.set(KEYS.map(|(vk, ..)| Toggle { on: unsafe { GetKeyState(vk.0 as i32) } & 1 != 0, down: false })));
    }
    let dev = RAWINPUTDEVICE {
        usUsagePage: 0x01, // Generic desktop.
        usUsage: 0x06,     // Keyboard.
        dwFlags: if hwnd.is_some() { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
        hwndTarget: hwnd.unwrap_or_default(),
    };
    // SAFETY: `dev` is a fully initialized device description that outlives the call.
    unsafe {
        if let Err(e) = RegisterRawInputDevices(&[dev], size_of::<RAWINPUTDEVICE>() as u32) {
            proto::error!("locks", "raw input: {e}");
        }
    }
}

/// Parses `WM_INPUT`: a notification if a toggle key was pressed.
///
/// The state changes on key press, like the system's own; auto-repeat while held is skipped
/// until release.
pub fn on_input(lparam: LPARAM) -> Option<Note> {
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    // SAFETY: `lparam` is the `HRAWINPUT` of the current `WM_INPUT`; `raw` is writable for `size` bytes.
    let read = unsafe { GetRawInputData(HRAWINPUT(lparam.0 as _), RID_INPUT, Some(&mut raw as *mut _ as _), &mut size, size_of::<RAWINPUTHEADER>() as u32) };
    if read == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
        return None;
    }
    // SAFETY: `dwType` was checked above, so the union holds keyboard data.
    let kb = unsafe { raw.data.keyboard };
    let i = KEYS.iter().position(|(vk, ..)| vk.0 == kb.VKey)?;
    let released = kb.Flags as u32 & RI_KEY_BREAK != 0;
    let on = STATE.with(|s| {
        let mut all = s.get();
        let t = &mut all[i];
        let toggled = !released && !t.down;
        t.down = !released;
        t.on ^= toggled;
        let on = t.on;
        s.set(all);
        toggled.then_some(on)
    })?;
    Some(note(i, on))
}

fn note(i: usize, on: bool) -> Note {
    let (_, key, name) = KEYS[i];
    let tr = i18n::t();
    Note {
        key: key.into(),
        source: None,
        app: None,
        popup: false,
        icon: Icon::Key,
        title: if on { name.into() } else { format!("{name} {}", tr.lock_short_off) },
        body: format!("{name} {}", if on { tr.lock_on } else { tr.lock_off }),
        meta: tr.keyboard.into(),
        ttl: Some(TTL),
        animated: false,
        swatches: Vec::new(),
        mark: None,
    }
}
