// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shell icons in the background: a shell query takes up to tens of milliseconds, and on the UI
//! thread the frame would wait for it. A result row appears at once, its icon when ready.
//!
//! Icons are looked up by shell parsing name: `shell:AppsFolder\…` for apps, a path for files.

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, mpsc},
};

use windows::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::HBITMAP,
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY},
    },
    core::{HSTRING, Interface},
};

/// Ready icons: an `HBITMAP` is a process-wide object, so it can cross threads.
static READY: Mutex<Option<HashMap<String, Option<isize>>>> = Mutex::new(None);
static PENDING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static WORKER: OnceLock<mpsc::Sender<(String, i32)>> = OnceLock::new();

/// Requests a `px`×`px` icon; `wake` wakes the UI thread when it is ready.
pub fn request(key: &str, px: i32, wake: fn()) {
    {
        let Ok(mut p) = PENDING.lock() else { return };
        if !p.get_or_insert_with(HashSet::new).insert(key.to_string()) {
            return;
        }
    }
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(String, i32)>();
        let _ = std::thread::Builder::new().name("icons".into()).spawn(move || {
            // Shell objects expect a single-threaded apartment.
            // SAFETY: initializes COM for this worker thread, once, before any shell call on it.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            for (key, px) in rx {
                let hbm = extract(&key, px).map(|h| h.0 as isize);
                if let Ok(mut r) = READY.lock() {
                    r.get_or_insert_with(HashMap::new).insert(key, hbm);
                }
                wake();
            }
        });
        tx
    });
    let _ = tx.send((key.to_string(), px));
}

/// Takes a ready icon: `Some(None)` means the item has no icon. The caller deletes the `HBITMAP`.
pub fn take(key: &str) -> Option<Option<HBITMAP>> {
    let hbm = READY.lock().ok()?.as_mut()?.remove(key)?;
    if let Ok(mut p) = PENDING.lock() {
        p.get_or_insert_with(HashSet::new).remove(key);
    }
    Some(hbm.map(|h| HBITMAP(h as _)))
}

fn extract(key: &str, px: i32) -> Option<HBITMAP> {
    // SAFETY: called on the icon thread after `CoInitializeEx`; `key` outlives the call.
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(key), None).ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        // Icon only: content thumbnails (pictures, videos) are not wanted in results.
        factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_ICONONLY).ok()
    }
}
