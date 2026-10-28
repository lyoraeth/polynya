// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop wallpaper watcher for the glass.
//!
//! No polling: the thread sleeps until Windows reports a write to `HKCU\Control Panel\Desktop`
//! (where the wallpaper path lives) or a file change in the wallpaper folder. The latter catches
//! slideshows and Spotlight, which change the picture without touching the path. The UI thread
//! checks what actually changed: the path and the file's modification time.

use std::path::Path;

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Storage::FileSystem::{FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification, FindFirstChangeNotificationW},
        System::{
            Registry::{HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW},
            Threading::{CreateEventW, INFINITE, WaitForMultipleObjects},
        },
    },
    core::{HSTRING, w},
};

/// Pause after an event: Windows writes the wallpaper in several steps, the first signal is not the last.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

/// Starts watching; `wake` is called from the background thread on every change.
pub fn watch(path: fn() -> Option<String>, wake: fn()) {
    // SAFETY: `key`, `event` and the change notification handle are opened in this thread,
    // used only here and closed once; `handles` outlives the wait.
    let _ = std::thread::Builder::new().name("wallpaper".into()).spawn(move || unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, w!(r"Control Panel\Desktop"), None, KEY_NOTIFY, &mut key).is_err() {
            return;
        }
        let Ok(event) = CreateEventW(None, false, false, None) else { return };
        loop {
            // The subscription fires once, so renew it every round.
            if RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, Some(event), true).is_err() {
                break;
            }
            let dir = path().and_then(|p| Path::new(&p).parent().map(|d| d.to_string_lossy().into_owned()));
            let files =
                dir.and_then(|d| FindFirstChangeNotificationW(&HSTRING::from(d), false, FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME).ok());
            let mut handles: Vec<HANDLE> = vec![event];
            handles.extend(files);
            let r = WaitForMultipleObjects(&handles, false, INFINITE);
            if let Some(f) = files {
                let _ = FindCloseChangeNotification(f);
            }
            if r.0 >= WAIT_OBJECT_0.0 + handles.len() as u32 {
                break;
            }
            std::thread::sleep(SETTLE);
            wake();
        }
        let _ = CloseHandle(event);
        let _ = RegCloseKey(key);
    });
}
