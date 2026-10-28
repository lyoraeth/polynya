// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Start menu replacement.
//!
//! Runs persistently (`polynya-start run`), communicating with the core via stdin/stdout.
//! Intercepts standalone Win key, Ctrl+Esc, and Start button clicks.
//! Injects a dummy key mask on Win release to prevent the native Windows Start menu from opening.

#![windows_subsystem = "windows"]

mod about;
mod menu;

use std::{
    cell::Cell,
    io::{BufRead, Write},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use proto::{ModIn, ModOut};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx},
            LibraryLoader::GetModuleHandleW,
            Threading::{GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
            Variant::VARIANT,
        },
        UI::{
            Accessibility::{CUIAutomation, HWINEVENTHOOK, IUIAutomation, SetWinEventHook, TreeScope_Descendants, UIA_AutomationIdPropertyId, UnhookWinEvent},
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL,
                VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                ASFW_ANY, AllowSetForegroundWindow, CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_OBJECT_SHOW,
                EVENT_SYSTEM_FOREGROUND, FindWindowW, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowRect, GetWindowThreadProcessId, HC_ACTION,
                KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, RegisterClassW, RegisterWindowMessageW, SW_HIDE,
                SetTimer, SetWindowsHookExW, ShowWindow, WH_KEYBOARD_LL, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT, WM_APP, WM_DISPLAYCHANGE, WM_KEYDOWN,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_QUIT, WM_SETTINGCHANGE, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, PWSTR, w},
};

const WM_APP_OPEN: u32 = WM_APP + 1;
const WM_APP_SELECT: u32 = WM_APP + 2;
const WM_APP_RECT: u32 = WM_APP + 3;
/// Unassigned virtual key code used to swallow the Win key release event.
const MASK_VK: u16 = 0xE8;
/// Fallback refresh interval for Start button bounds (ms), in case taskbar rebuilds silently.
const RECT_REFRESH_MS: u32 = 60_000;
/// Default fallback coordinates if Start button is not found.
const FALLBACK_AT: [i32; 2] = [0, 48];

static MAIN_TID: AtomicU32 = AtomicU32::new(0);
static SELECTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static START_RECT: Mutex<Option<RECT>> = Mutex::new(None);

thread_local! {
    /// Tracks if Win is held and if a combo key was pressed alongside it.
    static WIN: Cell<(bool, bool)> = const { Cell::new((false, false)) };
    /// Tracks if a mouse click on the Start button was swallowed.
    static CLICK: Cell<bool> = const { Cell::new(false) };
    /// Tracks if an Esc press from Ctrl+Esc was swallowed.
    static ESC: Cell<bool> = const { Cell::new(false) };
}

pub fn write(msg: &ModOut) {
    let Ok(line) = serde_json::to_string(msg) else { return };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}").and_then(|()| out.flush());
}

fn post(msg: u32) {
    // SAFETY: posting to a thread id has no memory-safety preconditions.
    unsafe {
        let _ = PostThreadMessageW(MAIN_TID.load(Ordering::Relaxed), msg, WPARAM(0), LPARAM(0));
    }
}

fn key(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
    }
}

unsafe extern "system" fn keyboard(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: called by the system for a low-level keyboard hook; for `HC_ACTION`, `lp` points to
    // a valid KBDLLHOOKSTRUCT for the duration of the call, and the inputs sent are fully initialized.
    unsafe {
        if code == HC_ACTION as i32 {
            let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            // Ignore injected events to prevent feedback loops.
            if !k.flags.contains(LLKHF_INJECTED) {
                let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                let vk = k.vkCode as u16;
                let (held, combo) = WIN.get();
                if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
                    if down {
                        if !held {
                            WIN.set((true, false));
                        }
                    } else {
                        WIN.set((false, false));
                        if held && !combo {
                            // Swallow the release and resend it after the mask: Windows sees "Win + mask"
                            // and does not open its own Start.
                            SendInput(&[key(MASK_VK, false), key(MASK_VK, true), key(vk, true)], size_of::<INPUT>() as i32);
                            post(WM_APP_OPEN);
                            return LRESULT(1);
                        }
                    }
                } else if held && down {
                    WIN.set((true, true));
                }
                // Intercept Ctrl+Esc (native Start), but allow Ctrl+Shift+Esc (Task Manager).
                if vk == VK_ESCAPE.0 {
                    let pressed = |k: VIRTUAL_KEY| GetAsyncKeyState(k.0 as i32) < 0;
                    if down && !held && pressed(VK_CONTROL) && !pressed(VK_SHIFT) && !pressed(VK_MENU) {
                        ESC.set(true);
                        post(WM_APP_OPEN);
                        return LRESULT(1);
                    }
                    if !down && ESC.get() {
                        ESC.set(false);
                        return LRESULT(1);
                    }
                }
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

unsafe extern "system" fn mouse(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: called by the system for a low-level mouse hook; for `HC_ACTION`, `lp` points to a
    // valid MSLLHOOKSTRUCT for the duration of the call.
    unsafe {
        if code == HC_ACTION as i32 {
            let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
            if m.flags & LLMHF_INJECTED == 0 {
                match wp.0 as u32 {
                    WM_LBUTTONDOWN if on_start(m.pt) => {
                        CLICK.set(true);
                        return LRESULT(1); // Swallow click
                    }
                    WM_LBUTTONUP if CLICK.get() => {
                        CLICK.set(false);
                        post(WM_APP_OPEN);
                        return LRESULT(1); // Swallow release
                    }
                    _ => {}
                }
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

fn on_start(pt: POINT) -> bool {
    START_RECT.lock().ok().and_then(|r| *r).is_some_and(|r| pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom)
}

/// Locates the Start button bounding rectangle via UI Automation.
fn find_start(uia: &IUIAutomation) -> Option<RECT> {
    // SAFETY: `uia` is a live COM object on this thread; the VARIANT outlives the call.
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()?;
        let root = uia.ElementFromHandle(tray).ok()?;
        let cond = uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("StartButton")).ok()?;
        let button = root.FindFirst(TreeScope_Descendants, &cond).ok()?;
        button.CurrentBoundingRectangle().ok().filter(|r| r.right > r.left)
    }
}

pub static MENU_OPEN: AtomicBool = AtomicBool::new(false);

/// Hides native XAML tooltips that fight for Z-order with the island when custom Start is open.
unsafe extern "system" fn popup_shown(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    // SAFETY: `hwnd` may be stale, which only makes the calls fail; `class` and `r` are valid out buffers.
    unsafe {
        if id_object != 0 || !MENU_OPEN.load(Ordering::Relaxed) {
            return;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        if String::from_utf16_lossy(&class[..n]) != "Xaml_WindowedPopupClass" {
            return;
        }
        let Some(start) = START_RECT.lock().ok().and_then(|r| *r) else { return };
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return;
        }
        // Only hide tooltips hovering directly over the Start button.
        let w = start.right - start.left;
        if r.right > start.left - w && r.left < start.right + w {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

/// Subscribes to window creation events specifically for the Explorer process.
fn watch_popups(hook: &mut Option<HWINEVENTHOOK>) {
    // SAFETY: the previous hook is ours and unhooked once; `popup_shown` matches the callback signature.
    unsafe {
        if let Some(h) = hook.take() {
            let _ = UnhookWinEvent(h);
        }
        let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) else { return };
        let mut pid = 0u32;
        GetWindowThreadProcessId(tray, Some(&mut pid));
        let h = SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW, None, Some(popup_shown), pid, 0, WINEVENT_OUTOFCONTEXT);
        *hook = (!h.is_invalid()).then_some(h);
    }
}

/// Checks if a given window belongs to the native Windows 11 Start Menu process.
fn is_native_start(hwnd: HWND) -> bool {
    // SAFETY: `buf` is writable for `len` characters; the process handle is closed exactly once.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok && String::from_utf16_lossy(&buf[..len as usize]).to_lowercase().ends_with(r"\startmenuexperiencehost.exe")
    }
}

/// Closes the custom Start menu if the native Start menu somehow gains foreground focus.
unsafe extern "system" fn foreground(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    if is_native_start(hwnd) {
        MENU_OPEN.store(false, Ordering::Relaxed);
        write(&ModOut::Close);
    }
}

/// Opens the custom Start menu, ensuring the island can receive keyboard focus. A second Win while
/// it is open closes it.
fn open() {
    if MENU_OPEN.swap(false, Ordering::Relaxed) {
        return write(&ModOut::Close);
    }
    // SAFETY: the inputs are fully initialized arrays that outlive `SendInput`.
    unsafe {
        // Close native Start if it's somehow open.
        if is_native_start(GetForegroundWindow()) {
            SendInput(&[key(VK_ESCAPE.0, false), key(VK_ESCAPE.0, true)], size_of::<INPUT>() as i32);
        }
        // Send input ourselves: only the process that received the last input may hand over the foreground.
        SendInput(&[key(MASK_VK, false), key(MASK_VK, true)], size_of::<INPUT>() as i32);
        // Required for the island to steal foreground and receive keyboard input after synthetic input.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let at = START_RECT.lock().ok().and_then(|r| *r).map_or(FALLBACK_AT, |r| [r.left, r.right]);
    MENU_OPEN.store(true, Ordering::Relaxed);
    write(&ModOut::Open { screen: menu::main_screen(), at: Some(at) });
}

/// Invisible window to receive broadcast messages (taskbar restart, resolution changes).
unsafe extern "system" fn listener(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: called by the system with valid arguments, which are passed on unchanged.
    unsafe {
        static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
        if msg == WM_DISPLAYCHANGE || msg == WM_SETTINGCHANGE || (msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed)) {
            post(WM_APP_RECT);
        }
        if TASKBAR_CREATED.load(Ordering::Relaxed) == 0 {
            TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() != Some("run") {
        return;
    }
    // SAFETY: runs on the main thread after COM initialization; hooks, the window and the message
    // loop live on this thread, and all buffers outlive their calls.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        MAIN_TID.store(GetCurrentThreadId(), Ordering::Relaxed);
        let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else { return };
        let refresh = || {
            let r = find_start(&uia);
            if let Ok(mut s) = START_RECT.lock() {
                *s = r;
            }
        };
        refresh();

        // Monitor stdin; EOF means core has exited.
        std::thread::spawn(|| {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                match serde_json::from_str(&line) {
                    Ok(ModIn::Select { id }) => {
                        if let Ok(mut q) = SELECTS.lock() {
                            q.push(id);
                        }
                        post(WM_APP_SELECT);
                    }
                    Ok(ModIn::Closed) => MENU_OPEN.store(false, Ordering::Relaxed),
                    Ok(ModIn::Submit { .. } | ModIn::Invoke { .. }) | Err(_) => {}
                }
            }
            post(WM_QUIT);
        });

        let Ok(instance) = GetModuleHandleW(None) else { return };
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(listener), hInstance: instance.into(), lpszClassName: w!("PolynyaStart"), ..Default::default() });
        let _ = CreateWindowExW(Default::default(), w!("PolynyaStart"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(instance.into()), None);
        let _ = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), Some(instance.into()), 0);
        let _ = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), Some(instance.into()), 0);
        SetTimer(None, 0, RECT_REFRESH_MS, None);
        let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(foreground), 0, 0, WINEVENT_OUTOFCONTEXT);
        let mut popups = None;
        watch_popups(&mut popups);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_APP_OPEN => open(),
                WM_APP_SELECT => {
                    let ids = SELECTS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
                    for id in ids {
                        menu::select(&id);
                    }
                }
                WM_APP_RECT | WM_TIMER => {
                    refresh();
                    watch_popups(&mut popups);
                }
                _ => {
                    DispatchMessageW(&msg);
                }
            };
        }
    }
}
