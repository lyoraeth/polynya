// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Apps from the system catalog `shell:AppsFolder`, the same one the Start menu uses: both
//! classic programs and Store apps. No own list is kept.
//!
//! The catalog is read in the background: at island start and after the launcher closes, if it is
//! stale, so a fresh one is ready by the next open, and the shell walk (hundreds of ms of CPU) never
//! overlaps the open animation. Installs and uninstalls are picked up automatically.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use windows::{
    Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree},
        UI::{
            Shell::{
                BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, KF_FLAG_DEFAULT, SHCreateItemFromParsingName,
                SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, ShellExecuteW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
    core::{HSTRING, PCWSTR},
};

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    /// Catalog name: an AppUserModelId or a path; the app is launched by it.
    pub id: String,
}

static CATALOG: Mutex<Option<Arc<Vec<App>>>> = Mutex::new(None);
/// Grows with each catalog read; tells that the results need recomputing.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static LOADING: AtomicBool = AtomicBool::new(false);

pub fn catalog() -> Arc<Vec<App>> {
    CATALOG.lock().ok().and_then(|c| c.clone()).unwrap_or_default()
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The catalog is reread no more often than this: walking all shell shortcuts is a noticeable CPU
/// spike, and installing programs is rare.
const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(300);
static LOADED_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Reads the catalog if it is still missing (prewarm did not make it), otherwise the launcher has
/// nothing to show.
pub fn ensure(done: fn()) {
    if LOADED_AT.lock().ok().and_then(|t| *t).is_none() {
        refresh(done);
    }
}

/// Rereads the catalog in the background if it is older than `REFRESH_EVERY`; `done` is called
/// from the background thread when ready.
pub fn refresh(done: fn()) {
    let fresh = LOADED_AT.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < REFRESH_EVERY);
    if fresh || LOADING.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::Builder::new().name("apps".into()).spawn(move || {
        // SAFETY: initializes COM for this new thread before any shell call on it.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let t = std::time::Instant::now();
        match enumerate() {
            Ok(apps) => {
                proto::debug!("launcher", "app catalog: {} apps in {} ms", apps.len(), t.elapsed().as_millis());
                if let Ok(mut c) = CATALOG.lock() {
                    *c = Some(Arc::new(apps));
                }
                GENERATION.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut t) = LOADED_AT.lock() {
                    *t = Some(std::time::Instant::now());
                }
                done();
            }
            Err(e) => proto::warn!("launcher", "app catalog: {e}"),
        }
        LOADING.store(false, Ordering::Relaxed);
    });
}

fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    // SAFETY: `item` is a live shell item; the returned string is freed exactly once.
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

fn enumerate() -> windows::core::Result<Vec<App>> {
    let mut out = Vec::new();
    // SAFETY: called on a thread with COM initialized; `batch` and `fetched` outlive each `Next`.
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        loop {
            let mut batch = [None];
            let mut fetched = 0u32;
            let _ = items.Next(&mut batch, Some(&mut fetched));
            let Some(item) = batch[0].take().filter(|_| fetched == 1) else { break };
            if let (Some(name), Some(id)) = (display_name(&item, SIGDN_NORMALDISPLAY), display_name(&item, SIGDN_PARENTRELATIVEPARSING)) {
                out.push(App { name, id });
            }
        }
    }
    Ok(out)
}

/// App's shell name; its icon is looked up by it.
pub fn shell_name(id: &str) -> String {
    format!(r"shell:AppsFolder\{id}")
}

/// Launches in the background: `ShellExecute` can be slow, and the close animation must not wait.
pub fn launch(id: &str) {
    open(shell_name(id));
}

/// Exe path of a classic program; Store apps have none, so `None`.
pub fn target_path(id: &str) -> Option<String> {
    // SAFETY: the HSTRING outlives the call; the returned string is freed exactly once.
    unsafe {
        let item: Option<IShellItem2> = SHCreateItemFromParsingName(&HSTRING::from(shell_name(id)), None).ok();
        let from_link = item.and_then(|item| {
            let p = item.GetString(&windows::Win32::Storage::EnhancedStorage::PKEY_Link_TargetParsingPath).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            s
        });
        // For some programs the catalog id is already the exe path.
        from_link.or_else(|| id.contains(":\\").then(|| id.to_string())).filter(|p| std::path::Path::new(p).is_file())
    }
}

/// Opens a file, folder or address in the background, like a double click in Explorer.
pub fn open(target: String) {
    shell("open", target, None);
}

/// Runs with an admin rights request (UAC).
pub fn run_as(path: String) {
    shell("runas", path, None);
}

/// Opens the file's folder in Explorer with the file selected.
pub fn reveal(path: String) {
    shell("open", "explorer.exe".into(), Some(format!("/select,\"{path}\"")));
}

fn shell(verb: &'static str, target: String, params: Option<String>) {
    // SAFETY: COM is initialized first on this new thread; all strings outlive `ShellExecuteW`.
    let _ = std::thread::Builder::new().name("launch".into()).spawn(move || unsafe {
        // ShellExecute requires a single-threaded apartment.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let params = params.map(HSTRING::from);
        let params = params.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr()));
        ShellExecuteW(None, &HSTRING::from(verb), &HSTRING::from(target), params, None, SW_SHOWNORMAL);
    });
}
