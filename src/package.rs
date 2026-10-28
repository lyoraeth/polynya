// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity package for modules that need package identity (Windows notifications).
//!
//! The package is unsigned and nearly empty: a manifest and a logo, while the module exe lives
//! outside, in the module folder (external location). It is installed once with admin rights
//! (`polynya notify-setup`: Windows 11 accepts an unsigned package with an executable only that
//! way) and removed without them. A process gets the identity only when launched through the
//! package, so the module is started by AUMID activation rather than directly.

use std::path::{Path, PathBuf};

use windows::{
    ApplicationModel::Package,
    Foundation::Uri,
    Management::Deployment::{AddPackageOptions, PackageManager},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Com::{CLSCTX_LOCAL_SERVER, CoCreateInstance},
            Threading::{INFINITE, WaitForSingleObject},
        },
        UI::{
            Shell::{
                ACTIVATEOPTIONS, ApplicationActivationManager, IApplicationActivationManager, IsUserAnAdmin, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
                ShellExecuteExW,
            },
            WindowsAndMessaging::SW_HIDE,
        },
    },
    core::{HSTRING, Result, w},
};

const NAME: &str = "Polynya.Notify";
const PUBLISHER: &str = "CN=Polynya, OID.2.25.311729368913984317654407730594956997722=1";
const APP_ID: &str = "Notify";
/// Version from `modules/notify/package/AppxManifest.xml`: an older installed one is reinstalled.
const VERSION: (u16, u16, u16, u16) = (1, 0, 1, 0);
static MSIX: &[u8] = include_bytes!("../modules/notify/polynya-notify.msix");

/// Package state on this machine.
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    /// Installed, but points to another folder or is an older version: needs reinstalling.
    Stale,
    /// Ready: the AUMID to launch.
    Ready(String),
}

fn installed() -> Option<Package> {
    let pm = PackageManager::new().ok()?;
    // Empty SID means the current user.
    pm.FindPackagesByUserSecurityIdNamePublisher(&HSTRING::new(), &HSTRING::from(NAME), &HSTRING::from(PUBLISHER)).ok()?.into_iter().next()
}

/// Package state for the module in `folder`.
pub fn state(folder: &Path) -> State {
    let Some(p) = installed() else { return State::Missing };
    let (Ok(id), Ok(path)) = (p.Id(), p.EffectiveExternalPath()) else { return State::Stale };
    let v = id.Version().unwrap_or_default();
    let same = PathBuf::from(path.to_string()) == folder;
    match id.FamilyName() {
        Ok(family) if same && (v.Major, v.Minor, v.Build, v.Revision) >= VERSION => State::Ready(format!("{family}!{APP_ID}")),
        _ => State::Stale,
    }
}

/// Installs the package for the module in `folder`, removing the previous one. Needs admin rights.
pub fn setup(folder: &Path) -> Result<()> {
    if installed().is_some() {
        remove()?;
    }
    let file = std::env::temp_dir().join("polynya-notify.msix");
    std::fs::write(&file, MSIX).map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    let opts = AddPackageOptions::new()?;
    opts.SetAllowUnsigned(true)?;
    opts.SetExternalLocationUri(&Uri::CreateUri(&HSTRING::from(uri(folder)))?)?;
    let result = PackageManager::new()?.AddPackageByUriAsync(&Uri::CreateUri(&HSTRING::from(uri(&file)))?, &opts)?.join();
    let _ = std::fs::remove_file(&file);
    result.map(|_| ())
}

fn uri(path: &Path) -> String {
    format!("file:///{}", path.display().to_string().replace('\\', "/"))
}

/// Removes the package. No admin rights needed.
pub fn remove() -> Result<()> {
    let Some(p) = installed() else { return Ok(()) };
    PackageManager::new()?.RemovePackageAsync(&p.Id()?.FullName()?)?.join().map(|_| ())
}

/// Launches the package app with arguments. Returns the pid.
pub fn launch(aumid: &str, args: &str) -> Result<u32> {
    // SAFETY: COM is initialized on the calling thread; the HSTRING arguments outlive the call.
    unsafe {
        let m: IApplicationActivationManager = CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)?;
        m.ActivateApplication(&HSTRING::from(aumid), &HSTRING::from(args), ACTIVATEOPTIONS(0))
    }
}

pub fn elevated() -> bool {
    // SAFETY: no arguments or preconditions.
    unsafe { IsUserAnAdmin().as_bool() }
}

/// Runs `polynya <args>` with admin rights (UAC prompt) and waits for it to finish.
/// `false` means UAC was declined or the launch failed.
pub fn run_elevated(args: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let (exe, args) = (HSTRING::from(exe.as_os_str()), HSTRING::from(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: windows::core::PCWSTR(exe.as_ptr()),
        lpParameters: windows::core::PCWSTR(args.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` is fully initialized, and `exe`/`args` back its string pointers for the whole
    // call; `hProcess` is valid after success and `wait` closes it.
    unsafe {
        if ShellExecuteExW(&mut info).is_err() || info.hProcess.is_invalid() {
            return false;
        }
        wait(info.hProcess);
    }
    true
}

/// Waits for a process and closes its handle.
pub fn wait(h: HANDLE) {
    // SAFETY: `h` is a process handle owned by the caller and closed exactly once here.
    unsafe {
        WaitForSingleObject(h, INFINITE);
        let _ = CloseHandle(h);
    }
}
