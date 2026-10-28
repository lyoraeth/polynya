// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Polynya "Windows Notifications" module: routes OS app notifications to the island.
//!
//! Requires package identity to access UserNotificationListener. Launched by the core
//! via the package manifest (`polynya-notify run <core_pid>`). Communicates with the
//! core via `proto::send` and monitors the core process for graceful shutdown.
//!
//! Usage:
//! - `polynya-notify run <core_pid>`
//! - `polynya-notify restore` (called by core to restore banners if module crashes)

#![windows_subsystem = "windows"]

mod banners;

use std::{collections::VecDeque, sync::mpsc};

use banners::Banner;
use proto::Msg;
use windows::{
    Foundation::TypedEventHandler,
    UI::Notifications::{
        KnownNotificationBindings,
        Management::{UserNotificationListener, UserNotificationListenerAccessStatus},
        UserNotification, UserNotificationChangedKind,
    },
    Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx},
            SystemInformation::GetLocalTime,
            Threading::{INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
    },
};

/// Lifetime of a notification chip in the zone (ms). Full history remains in OS Action Center.
const TTL_MS: u64 = 10_000;
/// Maximum number of simultaneous notification chips in the zone.
const MAX_SHOWN: usize = 5;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run(args.get(1).and_then(|p| p.parse().ok())),
        Some("restore") => banners::restore(),
        _ => {}
    }
}

enum Event {
    Added(u32),
    Removed(u32),
    Exit, // Core process terminated
}

fn run(core: Option<u32>) {
    // Restore state from potential previous crash
    banners::restore();
    if let Err(e) = listen(core) {
        proto::error!("notify", "listener: {e}");
    }
    banners::restore();
}

fn listen(core: Option<u32>) -> windows::core::Result<()> {
    // SAFETY: initializes COM for this thread before any WinRT call on it.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let listener = UserNotificationListener::Current()?;
    if listener.GetAccessStatus()? == UserNotificationListenerAccessStatus::Unspecified {
        listener.RequestAccessAsync()?.join()?;
    }
    if listener.GetAccessStatus()? != UserNotificationListenerAccessStatus::Allowed {
        proto::warn!("notify", "access to notifications denied");
        return Ok(());
    }
    let ours = ours();
    if ours {
        banners::take_known();
    }

    let (tx, rx) = mpsc::channel();
    let events = tx.clone();
    listener.NotificationChanged(&TypedEventHandler::new(
        move |_, args: windows::core::Ref<windows::UI::Notifications::UserNotificationChangedEventArgs>| {
            if let Some(a) = args.as_ref() {
                let id = a.UserNotificationId()?;
                let _ = events.send(if a.ChangeKind()? == UserNotificationChangedKind::Added { Event::Added(id) } else { Event::Removed(id) });
            }
            Ok(())
        },
    ))?;
    if let Some(pid) = core {
        watch(pid, tx);
    }

    let mut shown: VecDeque<u32> = VecDeque::new();
    for e in rx {
        match e {
            Event::Added(id) => {
                let Ok(n) = listener.GetNotification(id) else { continue };
                let Some(msg) = chip(&n, ours) else { continue };
                if proto::send(&msg).is_err() {
                    continue;
                }
                shown.push_back(id);
                while shown.len() > MAX_SHOWN {
                    let old = shown.pop_front().unwrap();
                    let _ = proto::send(&Msg::Dismiss { key: key(old) });
                }
            }
            Event::Removed(id) => {
                if let Some(i) = shown.iter().position(|&s| s == id) {
                    shown.remove(i);
                    let _ = proto::send(&Msg::Dismiss { key: key(id) });
                }
            }
            Event::Exit => break,
        }
    }
    Ok(())
}

/// Monitors the core process and signals exit when it terminates.
fn watch(pid: u32, tx: mpsc::Sender<Event>) {
    // SAFETY: `OpenProcess` has no memory-safety preconditions; failure is handled.
    let Ok(h) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        let _ = tx.send(Event::Exit);
        return;
    };
    let h = h.0 as usize;
    // SAFETY: the handle is moved into this thread, waited on and closed exactly once here.
    std::thread::spawn(move || unsafe {
        let h = windows::Win32::Foundation::HANDLE(h as *mut _);
        if WaitForSingleObject(h, INFINITE) == WAIT_OBJECT_0 {
            let _ = tx.send(Event::Exit);
        }
        let _ = CloseHandle(h);
    });
}

/// Checks config to determine if the island or the OS should render popups.
fn ours() -> bool {
    let text = std::fs::read_to_string(proto::data_dir().join("config.ini")).unwrap_or_default();
    let v = text.lines().filter_map(|l| l.split_once('=')).find(|(k, _)| k.trim() == "notify_banners").map(|(_, v)| v.trim().to_lowercase());
    v.as_deref() != Some("system")
}

fn key(id: u32) -> String {
    format!("notify:{id}")
}

/// Translates a Windows UserNotification into a Polynya chip message.
fn chip(n: &UserNotification, ours: bool) -> Option<Msg> {
    let app = n.AppInfo().ok()?;
    let aumid = app.AppUserModelId().ok()?.to_string();
    let name = app.DisplayInfo().and_then(|d| d.DisplayName()).map(|s| s.to_string()).unwrap_or_default();
    let texts: Vec<String> = n
        .Notification()
        .and_then(|x| x.Visual())
        .and_then(|v| v.GetBinding(&KnownNotificationBindings::ToastGeneric()?))
        .and_then(|b| b.GetTextElements())
        .map(|els| els.into_iter().filter_map(|e| e.Text().ok()).map(|s| s.to_string()).filter(|s| !s.trim().is_empty()).collect())
        .unwrap_or_default();
    let mut texts = texts.into_iter();
    let title = texts.next().unwrap_or_else(|| name.clone());
    let body = texts.collect::<Vec<_>>().join("\n");
    let banner = if ours { banners::take(&aumid) } else { Banner::System };
    proto::debug!("notify", "{aumid}: banner {banner:?}");
    // SAFETY: `GetLocalTime` has no preconditions.
    let t = unsafe { GetLocalTime() };
    Some(Msg::Chip {
        key: key(n.Id().ok()?),
        source: Some(format!("{aumid}\n{title}")),
        icon: "info".into(), // Core resolves actual app icon via AUMID
        meta: format!("{name} · {:02}:{:02}", t.wHour, t.wMinute),
        title,
        body,
        ttl_ms: Some(TTL_MS),
        animated: false,
        app: Some(aumid),
        // Trigger island popup only if we successfully disabled the native system banner
        popup: banner == Banner::Ours,
        swatches: Vec::new(),
        mark: None,
    })
}
