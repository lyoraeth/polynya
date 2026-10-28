// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Permanent exe location: `%LOCALAPPDATA%\Polynya\polynya.exe`.
//!
//! Autostart and agent hooks store the exe path. Run from a downloads or build folder, the island
//! would leave a path there that goes stale once the file is deleted or rebuilt. So a launch from
//! elsewhere copies the exe there and hands over to the copy; a running older instance is stopped.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM},
        System::Threading::{OpenMutexW, ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject},
        UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE},
    },
    core::PCWSTR,
};

/// How long to wait for the old instance to close and release the file.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("Polynya").join("polynya.exe"))
}

/// Moves the launch to the permanent place. `true`: the copy took over and this process should
/// exit; `false`: the island is already in place or the move failed (it then runs from where it
/// was started; the reason is in the log).
pub fn relocate(class: PCWSTR, mutex: PCWSTR) -> bool {
    // A portable copy runs where it lies.
    if proto::portable_root().is_some() {
        return false;
    }
    let (Ok(cur), Some(dst)) = (std::env::current_exe(), path()) else { return false };
    if same_file(&cur, &dst) {
        // The previous version, set aside during replacement, has exited by now.
        let _ = std::fs::remove_file(dst.with_extension("old"));
        return false;
    }
    if let Some(from) = cur.parent()
        && let Err(e) = crate::modules::bundle(from)
    {
        proto::error!("install", "bundle modules: {e}");
    }
    match install(&cur, &dst, class, mutex) {
        Ok(()) => match std::process::Command::new(&dst).spawn() {
            Ok(_) => true,
            Err(e) => {
                proto::error!("install", "start {}: {e}", dst.display());
                false
            }
        },
        Err(e) => {
            proto::error!("install", "copy to {}: {e}", dst.display());
            false
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Puts the exe in place. If the same version is already there, touches nothing: the copy exits
/// on launch by itself if the island is already running.
fn install(cur: &Path, dst: &Path, class: PCWSTR, mutex: PCWSTR) -> std::io::Result<()> {
    let bytes = std::fs::read(cur)?;
    if std::fs::read(dst).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    stop(class, mutex);
    std::fs::create_dir_all(dst.parent().unwrap())?;
    // A running exe cannot be overwritten but can be renamed, in case the old instance has not
    // exited yet.
    let old = dst.with_extension("old");
    let _ = std::fs::remove_file(&old);
    if dst.exists() {
        std::fs::rename(dst, &old)?;
    }
    std::fs::write(dst, bytes)
}

/// Asks the running island to exit and waits until it releases the instance mutex, at most
/// [`STOP_TIMEOUT`]. Does nothing when the island is not running.
pub fn stop(class: PCWSTR, mutex: PCWSTR) {
    // SAFETY: `mutex` and `class` are valid null-terminated strings from the caller; `h` is
    // closed exactly once.
    unsafe {
        let Ok(h) = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, mutex) else { return };
        if let Ok(hwnd) = FindWindowW(class, PCWSTR::null()) {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let r = WaitForSingleObject(h, STOP_TIMEOUT.as_millis() as u32);
        // A process that exited without releasing the mutex gives WAIT_ABANDONED; that is a success too.
        if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
            let _ = ReleaseMutex(h);
        }
        let _ = CloseHandle(h);
    }
}
