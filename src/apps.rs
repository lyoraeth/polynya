// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Matches a media session's app to a process.
//!
//! A media session reports the app's AppUserModelId, the same id Windows stores on the app's
//! windows. So the main path does not depend on specific programs: find a visible window with the
//! same id and take its process. This works for any browser or player without lists of known apps.

use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS, HWND, LPARAM},
        Storage::{EnhancedStorage::PKEY_AppUserModel_ID, Packaging::Appx::GetApplicationUserModelId},
        System::{
            Com::{CoTaskMemFree, StructuredStorage::PropVariantToStringAlloc},
            Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS},
            Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
        },
        UI::{
            Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
            WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
        },
    },
    core::{BOOL, PWSTR},
};

pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Lowercase exe name.
    pub exe: String,
}

pub fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    // SAFETY: `e` is initialized with its `dwSize` and outlives each call; `snap` is closed once.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let exe = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
            out.push(Proc { pid: e.th32ProcessID, parent: e.th32ParentProcessID, exe });
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Full lowercase exe path of the process; empty if the process could not be opened.
pub fn process_path(pid: u32) -> String {
    // SAFETY: `buf` is writable for `len` characters; `h` is closed exactly once.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok { String::from_utf16_lossy(&buf[..len as usize]).to_lowercase() } else { String::new() }
    }
}

/// App id from the package (Store apps).
fn packaged_app_id(pid: u32) -> Option<String> {
    // SAFETY: `buf` is writable for `len` characters; `h` is closed exactly once.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let res = GetApplicationUserModelId(h, &mut len, Some(PWSTR(buf.as_mut_ptr())));
        let _ = CloseHandle(h);
        // The length includes the terminating null.
        (res == ERROR_SUCCESS && len > 1).then(|| String::from_utf16_lossy(&buf[..len as usize - 1]))
    }
}

/// Explicit app id assigned to the window (browsers and many players do this).
fn window_app_id(hwnd: HWND) -> Option<String> {
    // SAFETY: `hwnd` may be stale, which only makes the call fail; the returned string is freed once.
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let p = PropVariantToStringAlloc(&value).ok()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        (!s.is_empty()).then_some(s)
    }
}

unsafe extern "system" fn collect_visible(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut Vec<HWND>` passed by `by_window`, alive for the whole enumeration.
    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
        }
    }
    BOOL(1)
}

/// Whether the ids match: browsers append the profile to the window id after a dot
/// ("Chrome.ProfileX"), while the media session reports it without the suffix, or vice versa.
fn same_app(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    a == b || a.starts_with(&format!("{b}.")) || b.starts_with(&format!("{a}."))
}

/// Process of a visible window with the same app id.
fn by_window(app: &str) -> Option<u32> {
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: `windows` outlives the synchronous enumeration that writes to it via `lparam`.
    unsafe {
        let _ = EnumWindows(Some(collect_visible), LPARAM(&mut windows as *mut _ as isize));
    }
    windows.into_iter().find_map(|hwnd| {
        let mut pid = 0u32;
        // SAFETY: `pid` is a valid out pointer for the call.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let id = window_app_id(hwnd).or_else(|| packaged_app_id(pid))?;
        same_app(&id, app).then_some(pid)
    })
}

/// Climbs to the topmost ancestor with the same exe: in browsers the window belongs to one process
/// and another child plays the sound, so capturing the tree from the root covers all of them.
fn root_of(procs: &[Proc], pid: u32) -> Option<&Proc> {
    let mut cur = procs.iter().find(|p| p.pid == pid)?;
    while let Some(parent) = procs.iter().find(|p| p.pid == cur.parent && p.exe == cur.exe) {
        cur = parent;
    }
    Some(cur)
}

/// Root process of the media session's app: exe name and pid.
///
/// Order: a window with the same id → an exe name right in the id (classic programs without an
/// explicit id) → a folder named after the app in the exe path (the player is minimized to the
/// tray and shows no windows).
pub fn find_process(app: &str) -> Option<(String, u32)> {
    let procs = processes();
    let pid = by_window(app).or_else(|| {
        let a = app.to_lowercase();
        if a.ends_with(".exe") {
            let exe = a.rsplit(['\\', '/']).next().unwrap_or(&a).to_string();
            procs.iter().find(|p| p.exe == exe).map(|p| p.pid)
        } else {
            let folder = format!("\\{a}\\");
            procs.iter().find(|p| process_path(p.pid).contains(&folder)).map(|p| p.pid)
        }
    })?;
    let root = root_of(&procs, pid)?;
    Some((root.exe.clone(), root.pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_match_with_profile_suffix() {
        assert!(same_app("Chrome", "chrome"));
        assert!(same_app("Chrome.UserData.Profile1", "Chrome"));
        assert!(same_app("Spotify.exe", "Spotify.exe"));
        assert!(!same_app("Chrome", "Chromium"));
        assert!(!same_app("Helium", "Chrome"));
    }

    #[test]
    fn root_climbs_same_exe_only() {
        let procs = vec![
            Proc { pid: 1, parent: 0, exe: "explorer.exe".into() },
            Proc { pid: 10, parent: 1, exe: "chrome.exe".into() },
            Proc { pid: 11, parent: 10, exe: "chrome.exe".into() },
            Proc { pid: 12, parent: 11, exe: "chrome.exe".into() },
        ];
        assert_eq!(root_of(&procs, 12).map(|p| p.pid), Some(10));
        assert_eq!(root_of(&procs, 1).map(|p| p.pid), Some(1));
    }
}
