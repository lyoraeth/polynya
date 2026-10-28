// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Manages system notification banners for applications tracked by the island.
//!
//! While the module is active, it disables native banners (`ShowBanner` = 0) for apps
//! whose notifications are displayed in the island, preventing duplicate popups.
//! Original values are backed up and restored on exit. If the process crashes,
//! a `RunOnce` registry entry ensures restoration via system `reg import` on next login.
//! Apps with banners manually disabled by the user are left untouched.

use std::{fs, path::PathBuf};

use windows::{
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW},
    },
    core::{HSTRING, PCWSTR, w},
};

const SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings";
const RUN_ONCE: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\RunOnce");
const RUN_ONCE_NAME: PCWSTR = w!("PolynyaBanners");
const VALUE: PCWSTR = w!("ShowBanner");

/// Ownership status of an application's system banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banner {
    /// Disabled by this module; the island will show the popup.
    Ours,
    /// Native system state: user manually disabled it or restored it.
    System,
}

/// Backup entry: App AUMID and its original `ShowBanner` value (None if absent).
type Entry = (String, Option<u32>);

fn backup_path() -> PathBuf {
    proto::data_dir().join("banners.txt")
}

/// List of apps whose banners are currently managed by the island (one AUMID per line).
fn known_path() -> PathBuf {
    proto::data_dir().join("banners-known.txt")
}

fn known() -> Vec<String> {
    std::fs::read_to_string(known_path()).map(|t| t.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
}

fn set_known(aumid: &str, on: bool) {
    let mut list = known();
    let has = list.iter().any(|a| a == aumid);
    if on == has {
        return;
    }
    if on {
        list.push(aumid.to_string());
    } else {
        list.retain(|a| a != aumid);
    }
    let _ = fs::create_dir_all(proto::data_dir());
    let _ = fs::write(known_path(), list.join("\n"));
}

/// Disables banners for all previously known apps on module startup,
/// preventing their next system notification from duplicating in the island.
pub fn take_known() {
    for aumid in known() {
        take(&aumid);
    }
}

fn reg_path() -> PathBuf {
    proto::data_dir().join("banners-restore.reg")
}

fn key(aumid: &str) -> HSTRING {
    HSTRING::from(format!(r"{SETTINGS}\{aumid}"))
}

fn read(aumid: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut size = 4u32;
    // SAFETY: `v` is writable for `size` (4) bytes; the key HSTRING outlives the call.
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &key(aumid), VALUE, RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut size)) };
    (r == ERROR_SUCCESS).then_some(v)
}

fn write(aumid: &str, v: Option<u32>) -> bool {
    let r = match v {
        // SAFETY: `v` lives on this frame and is exactly the 4 bytes passed; the key HSTRING
        // outlives the call.
        Some(v) => unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, &key(aumid), VALUE, REG_DWORD.0, Some(&v as *const u32 as *const _), 4) },
        // SAFETY: the key HSTRING outlives the call.
        None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &key(aumid), VALUE) },
    };
    // Succeeds if value is written or successfully confirmed absent
    r == ERROR_SUCCESS || (v.is_none() && read(aumid).is_none())
}

fn load() -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(backup_path()) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let (aumid, v) = l.rsplit_once('\t')?;
            Some((aumid.to_string(), if v == "-" { None } else { Some(v.parse().ok()?) }))
        })
        .collect()
}

/// Saves the backup and the `RunOnce` crash recovery entry.
/// If entries are empty, removes both artifacts.
fn save(entries: &[Entry]) -> bool {
    if entries.is_empty() {
        let _ = fs::remove_file(backup_path());
        let _ = fs::remove_file(reg_path());
        // SAFETY: the key and value names are static strings.
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_ONCE, RUN_ONCE_NAME);
        }
        return true;
    }
    let _ = fs::create_dir_all(proto::data_dir());
    let text: String = entries.iter().map(|(a, v)| format!("{a}\t{}\n", v.map_or("-".into(), |v| v.to_string()))).collect();
    if fs::write(backup_path(), text).is_err() {
        return false;
    }
    // .reg file must be UTF-16 LE with BOM for `reg import` compatibility
    let mut reg = String::from("Windows Registry Editor Version 5.00\r\n");
    for (aumid, v) in entries {
        let value = v.map_or("-".into(), |v| format!("dword:{v:08x}"));
        reg.push_str(&format!("\r\n[HKEY_CURRENT_USER\\{SETTINGS}\\{aumid}]\r\n\"ShowBanner\"={value}\r\n"));
    }
    let bytes: Vec<u8> = [0xFEFFu16].into_iter().chain(reg.encode_utf16()).flat_map(u16::to_le_bytes).collect();
    if fs::write(reg_path(), bytes).is_err() {
        return false;
    }
    let cmd: Vec<u16> = format!("reg.exe import \"{}\"", reg_path().display()).encode_utf16().chain([0]).collect();
    // SAFETY: `cmd` is a null-terminated UTF-16 buffer of exactly the byte length passed, alive for the call.
    let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, RUN_ONCE, RUN_ONCE_NAME, REG_SZ.0, Some(cmd.as_ptr() as *const _), (cmd.len() * 2) as u32) };
    r == ERROR_SUCCESS
}

/// Evaluates and takes ownership of an app's banner upon receiving its first notification.
pub fn take(aumid: &str) -> Banner {
    let mut entries = load();
    let current = read(aumid);
    if let Some(i) = entries.iter().position(|(a, _)| a == aumid) {
        if current == Some(0) {
            return Banner::Ours;
        }
        // User manually re-enabled banners in OS settings; respect their choice
        entries.remove(i);
        save(&entries);
        set_known(aumid, false);
        return Banner::System;
    }
    if current == Some(0) {
        // Already disabled by user
        set_known(aumid, false);
        return Banner::System;
    }
    // Backup original value before modifying registry
    entries.push((aumid.to_string(), current));
    if !save(&entries) {
        return Banner::System;
    }
    if !write(aumid, Some(0)) {
        return Banner::System;
    }
    set_known(aumid, true);
    Banner::Ours
}

/// Restores banners to their original state.
/// Leaves values that the user has manually changed since the backup.
/// Failed restorations remain in the backup for the next attempt.
pub fn restore() {
    let entries = load();
    if entries.is_empty() {
        return;
    }
    let failed: Vec<Entry> = entries.into_iter().filter(|(aumid, v)| read(aumid) == Some(0) && !write(aumid, *v)).collect();
    save(&failed);
}
