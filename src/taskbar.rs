// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Taskbar height detection.
//!
//! Source order: config value → taskbar position from the system → default. Mods that resize the
//! taskbar (Windhawk `taskbar-icon-size`) apply after Explorer starts, so the height is rechecked
//! when the work area changes, and the window is recreated if it changed.

use std::sync::Mutex;

use windows::{
    Win32::{
        Foundation::RECT,
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
            Variant::VARIANT,
        },
        UI::Shell::{ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABM_GETTASKBARPOS, APPBARDATA, SHAppBarMessage},
        UI::{
            Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants, UIA_AutomationIdPropertyId},
            WindowsAndMessaging::FindWindowW,
        },
    },
    core::{PCWSTR, w},
};

/// How often the icon area is rechecked: the centered group spreads as programs open.
const FREE_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// Free left part of the taskbar with centered icons, screen pixels: `(left, right)`.
/// `None`: icons are on the left and the middle is free.
static FREE: Mutex<Option<(i32, i32)>> = Mutex::new(None);

pub fn free() -> Option<(i32, i32)> {
    FREE.lock().ok().and_then(|f| *f)
}

/// Whether the taskbar is light: the "Windows mode" in personalization settings (apps have their
/// own, but the island is part of the taskbar).
pub fn light() -> bool {
    let mut v = 0u32;
    let mut size = 4u32;
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    // SAFETY: `v` is writable for `size` (4) bytes; the key and value names are static strings.
    let ok =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, key, w!("SystemUsesLightTheme"), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut size)) }
            .is_ok();
    ok && v != 0
}

/// Centered icons: the "Taskbar alignment" setting; the Windows 11 default.
fn centered() -> bool {
    let mut v = 1u32;
    let mut size = 4u32;
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced");
    // SAFETY: `v` is writable for `size` (4) bytes; the key and value names are static strings.
    let ok =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, key, w!("TaskbarAl"), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut size)) }.is_ok();
    !ok || v != 0
}

fn find(uia: &IUIAutomation, root: &IUIAutomationElement, id: &str) -> Option<IUIAutomationElement> {
    // SAFETY: `uia` and `root` are live COM objects on this thread; the VARIANT outlives the call.
    unsafe {
        let cond = uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from(id)).ok()?;
        root.FindFirst(TreeScope_Descendants, &cond).ok()
    }
}

fn rect(e: &IUIAutomationElement) -> Option<RECT> {
    // SAFETY: `e` is a live UI Automation element.
    unsafe { e.CurrentBoundingRectangle() }.ok().filter(|r| r.right > r.left)
}

/// Found Start and Widgets buttons. Searching the taskbar tree is expensive, so it is done once;
/// after that only the button rectangle is read every second.
#[derive(Default)]
struct Found {
    start: Option<IUIAutomationElement>,
    widgets: Option<IUIAutomationElement>,
}

/// The free left part right now: from Widgets (if on the left) to the Start button, which is the
/// first in the icon group when centered.
fn measure(uia: &IUIAutomation, found: &mut Found) -> Option<(i32, i32)> {
    if !centered() {
        return None;
    }
    // SAFETY: runs on the COM-initialized taskbar thread; `bar` outlives the call.
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()?;
        // The button is gone (Explorer restarted): search again.
        if found.start.as_ref().and_then(rect).is_none() {
            let root = uia.ElementFromHandle(tray).ok()?;
            found.start = find(uia, &root, "StartButton");
            found.widgets = find(uia, &root, "WidgetsButton");
        }
        let start = rect(found.start.as_ref()?)?;
        let mut bar = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(tray, &mut bar);
        let left = found.widgets.as_ref().and_then(rect).filter(|w| w.right <= start.left).map_or(bar.left, |w| w.right);
        Some((left, start.left))
    }
}

/// Watches the free area in a background thread; `wake` means it changed.
pub fn watch(wake: fn()) {
    // SAFETY: COM is initialized first on this new thread, and `uia` is used only on it.
    let _ = std::thread::Builder::new().name("taskbar".into()).spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else { return };
        let mut found = Found::default();
        loop {
            let now = measure(&uia, &mut found);
            let changed = FREE.lock().map(|mut f| std::mem::replace(&mut *f, now) != now).unwrap_or(false);
            if changed {
                wake();
            }
            std::thread::sleep(FREE_POLL);
        }
    });
}

const FALLBACK: f32 = 48.0;

/// Which screen edge the taskbar is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    /// Left or right: the island does not live in a vertical taskbar, only standalone.
    Side,
}

pub fn edge() -> Edge {
    let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, ..Default::default() };
    // SAFETY: `abd` is initialized with its `cbSize` and outlives the call.
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) } == 0 {
        return Edge::Top;
    }
    match abd.uEdge {
        e if e == ABE_BOTTOM => Edge::Bottom,
        e if e == ABE_LEFT || e == ABE_RIGHT => Edge::Side,
        _ => Edge::Top,
    }
}

/// Taskbar height in DIPs. `scale` is the monitor scale (DPI / 96).
pub fn height(config_override: Option<f32>, scale: f32) -> f32 {
    config_override.or_else(|| system(scale)).unwrap_or(FALLBACK)
}

fn system(scale: f32) -> Option<f32> {
    let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, ..Default::default() };
    // SAFETY: `abd` is initialized with its `cbSize` and outlives the call.
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) } == 0 {
        return None;
    }
    let RECT { top, bottom, .. } = abd.rc;
    Some((bottom - top) as f32 / scale).filter(|h| *h > 0.0)
}
