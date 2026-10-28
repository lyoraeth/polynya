// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Autostart at sign-in via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

use windows::{
    Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    },
    core::{PCWSTR, w},
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE: PCWSTR = w!("Polynya");

fn exe_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

/// Whether autostart is enabled for this exact executable.
pub fn is_enabled() -> bool {
    let Some(cmd) = exe_command() else { return false };
    registered().is_some_and(|r| r.eq_ignore_ascii_case(&cmd))
}

/// Points autostart registered for another path (the exe moved to its permanent place)
/// to the current exe.
pub fn refresh() {
    // A portable copy must not take over the autostart of an installed one.
    if proto::portable_root().is_some() {
        return;
    }
    let Some(cmd) = registered() else { return };
    let ours = cmd.to_lowercase().trim_matches('"').ends_with("polynya.exe");
    if ours && !is_enabled() {
        set(true);
    }
}

fn registered() -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size = size_of_val(&buf) as u32;
    // SAFETY: `buf` is writable for `size` bytes, and `size` holds its real byte length.
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)) }.ok().ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn set(enabled: bool) {
    // SAFETY: `key` is opened before use and closed once; `bytes` views `wide`, which outlives
    // the call, and its length is exactly the UTF-16 buffer size in bytes.
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE | KEY_READ, &mut key).is_err() {
            return;
        }
        if enabled {
            if let Some(cmd) = exe_command() {
                let wide: Vec<u16> = cmd.encode_utf16().chain([0]).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                let _ = RegSetValueExW(key, VALUE, None, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, VALUE);
        }
        let _ = RegCloseKey(key);
    }
}
