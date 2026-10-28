// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Polynya: an island in the middle of the top taskbar.

#![windows_subsystem = "windows"]

mod a11y;
mod apps;
mod audio;
mod autostart;
mod cli;
mod config;
mod edit;
mod gfx;
mod hotkeys;
mod i18n;
mod icons;
mod install;
mod ipc;
mod island;
mod launcher;
mod locks;
mod look;
mod media;
mod modules;
mod package;
mod settings;
mod shape;
mod spring;
mod taskbar;
mod terminal;
mod wallpaper;
mod weather;

use std::{
    cell::{Cell, OnceCell, RefCell},
    sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering},
    time::Instant,
};

use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, RGN_OR, SetWindowRgn,
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            LibraryLoader::GetModuleHandleW,
            SystemInformation::GetLocalTime,
            Threading::{CreateMutexW, GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook},
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::{
                GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, MOD_ALT, MOD_NOREPEAT, RegisterHotKey, SendInput,
                TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, UnregisterHotKey, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_N, VK_SHIFT, VK_SPACE,
            },
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, QUNS_ACCEPTS_NOTIFICATIONS, QUNS_PRESENTATION_MODE,
                QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState, Shell_NotifyIconW, ShellExecuteW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{HSTRING, PCWSTR, Result, w},
};

use config::Config;
use island::{Action, Island, Panel, Part};

/// Background frame rate: visualizer and marquee. Independent of the monitor: on 144 Hz
/// "every other refresh" would give 72 frames where 30 are enough.
const BACKGROUND_FRAME: std::time::Duration = std::time::Duration::from_micros(33_333);

/// Window width in DIPs: with room for the widest panel shifted toward the pill's edge.
/// Window height headroom for panels.
const PANEL_SPACE: f32 = 260.0;
/// Wallpaper blur for the glass (sigma, DIP) and how much the picture is downscaled: a small
/// blurred picture looks the same at tens of times less memory and work.
const GLASS_BLUR: f32 = 40.0;
const GLASS_SCALE: f32 = 0.125;
/// Height of the wallpaper band near the taskbar used for the Mica average tone, DIP.
const NEAR_BAND: f32 = 300.0;
/// The window grows for a panel with headroom for spring overshoot and the bottom corner, in steps,
/// so the surface is not recreated every few pixels.
const GROW_OVERSHOOT: f32 = 1.12;
const GROW_PAD: f32 = 30.0;
const GROW_STEP: f32 = 40.0;

pub(crate) const CLASS: PCWSTR = w!("Polynya");

const WM_APP_TRAY: u32 = WM_APP + 1;
const WM_APP_OUTSIDE: u32 = WM_APP + 2;
/// Thread message: the window was destroyed against our will and must be recreated.
const WM_APP_RESPAWN: u32 = WM_APP + 3;
/// The media session state changed.
const WM_APP_MEDIA: u32 = WM_APP + 4;
/// The wallpaper may have changed; from the watcher thread.
const WM_APP_WALLPAPER: u32 = WM_APP + 20;
/// The foreground window changed: time to check fullscreen mode.
const WM_APP_FOREGROUND: u32 = WM_APP + 5;
/// A message arrived over the pipe (`polynya push`, a Claude Code hook).
const WM_APP_PUSH: u32 = WM_APP + 7;
/// A command finished in a watched terminal.
const WM_APP_TERM: u32 = WM_APP + 8;
/// The cursor left the window; the constant lives in Win32_UI_Controls, not worth the feature for one value.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Recapture the system look: `polynya refresh`. The taskbar was recolored behind Windows' back
/// (e.g. by a Windhawk patch), with no event about it.
pub const WM_APP_REFRESH: u32 = WM_APP + 21;
/// The identity package was installed (`polynya notify-setup`): start the modules waiting for it.
pub const WM_APP_PACKAGE: u32 = WM_APP + 22;
/// Restart module processes: `polynya modules reload`.
pub const WM_APP_MODULES: u32 = WM_APP + 23;
/// The selection for a module hotkey was copied: `hotkeys::take`.
const WM_APP_CAPTURED: u32 = WM_APP + 24;
/// The accessibility tree changed: send it to the screen reader, see `a11y`.
const WM_APP_A11Y_SYNC: u32 = WM_APP + 25;
/// A screen reader pressed a control; `wParam` is its id.
const WM_APP_A11Y_ACTION: u32 = WM_APP + 26;
/// The session was unlocked; constant from Win32_System_RemoteDesktop.
const WTS_SESSION_UNLOCK: usize = 8;
/// Resume from sleep.
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const TIMER_CLOCK: usize = 1;
/// Deferred window recreation: display change events come in bursts, so coalesce them.
const TIMER_REBUILD: usize = 2;
const REBUILD_DEBOUNCE_MS: u32 = 300;
/// Deadline of the nearest right-zone notification.
const TIMER_NOTES: usize = 3;
/// Next picture change without continuous animation: the Claude logo glyph.
const TIMER_WAKE: usize = 4;
/// Launcher hotkey: Alt+Space.
const HOTKEY_LAUNCHER: i32 = 1;
/// Processes finished commands with a delay: the chain's next command does not start at once.
const TIMER_TERM: usize = 5;

const TERM_SETTLE_MS: u32 = 300;
/// System look capture after events that change the taskbar. Theme and wallpaper changes are
/// animated and events come in bursts, so capture once everything has settled.
const TIMER_LOOK: usize = 6;
const LOOK_SETTLE_MS: u32 = 1000;
/// How long closes are ignored after the unapplied settings warning is shown.
const GUARD_GRACE: std::time::Duration = std::time::Duration::from_millis(400);
/// Follow-up captures after the window starts, ms from creation: after a cold boot Explorer draws
/// the taskbar backdrop later than the island starts, so the first capture comes out almost black,
/// and there is no event for the backdrop being ready.
const TIMER_LOOK_BOOT: usize = 7;
const LOOK_BOOT_MS: [u32; 4] = [2000, 5000, 15000, 30000];
/// Search prewarm after startup, once everything else has settled, ms.
const TIMER_PREWARM: usize = 8;
/// The launcher waits this long for a module to answer its `invoke` before closing, ms.
const TIMER_INVOKE: usize = 9;
const INVOKE_WAIT_MS: u32 = 1000;
const PREWARM_MS: u32 = 10_000;
/// Window recreation retry while the taskbar has not appeared yet, ms.
const RESPAWN_RETRY_MS: u32 = 1000;

const ID_SETTINGS: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_EXIT: usize = 3;
const ID_REFRESH: usize = 4;

/// The island window, for hooks that have no context of their own.
static HWND_ISLAND: AtomicIsize = AtomicIsize::new(0);
/// The user chose "Exit": destroying the window ends the app instead of recreating the window.
static QUITTING: AtomicBool = AtomicBool::new(false);
/// The island is hidden under a fullscreen app and must not be raised.
static HIDDEN: AtomicBool = AtomicBool::new(false);
/// Long terminal command threshold, s; 0 disables watching.
static TERMINAL_MIN: AtomicU64 = AtomicU64::new(0);
/// Process of the previous foreground window: the one the user left.
static LAST_FOREGROUND: AtomicU32 = AtomicU32::new(0);

struct App {
    hwnd: HWND,
    gfx: gfx::Gfx,
    island: Island,
    scale: f32,
    /// Bottom taskbar: the window sticks to the bottom edge and grows upward.
    bottom: bool,
    /// The monitor the window was built on.
    monitor: RECT,
    /// How the taskbar looks right now, plus the Windows theme and accent.
    look: Option<look::Look>,
    /// When the unapplied settings warning was shown.
    guard_at: Option<Instant>,
    /// Theme from the config, and the one the window was built in.
    theme: config::Theme,
    light: bool,
    /// Glass from the wallpaper: background of dropdowns and the glass capsule.
    glass: Option<Glass>,
    /// Height of the band with blocks from the window edge: the taskbar or the capsule with its inset.
    bar_h: f32,
    /// Window's left edge on screen, pixels: for spans modules give in screen coordinates.
    left_px: i32,
    /// Taskbar height the window was built for, and the manual height from the config.
    taskbar_h: f32,
    taskbar_override: Option<f32>,
    last: Instant,
    animating: bool,
    /// Low-level mouse hook thread, while a panel is open.
    hook: Option<u32>,
    minute: u16,
    taskbar_created: u32,
    /// The cursor is over the window: the `WM_MOUSELEAVE` subscription is active.
    tracking_mouse: bool,
    /// The last window region set, physical pixels.
    region: Vec<(i32, i32, i32, i32)>,
    /// The graphics stack is broken (device lost): no drawing until the window is recreated.
    broken: bool,
    /// The window the launcher was opened from; Esc returns focus to it.
    prev_focus: Option<HWND>,
    /// Module whose command was just run from search or a hotkey: its `open` from search replaces
    /// the launcher as a page.
    invoking: Option<String>,
    /// The open module dropdown answers a command: its `close` returns focus to the window the
    /// command came from (the user pastes the result there).
    menu_restore: bool,
    /// When to draw the next background frame; `None` means now (an animation is running).
    next_frame: Option<Instant>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// Media sessions outlive the window: recreating the window does not restart the thread.
    static MEDIA: OnceCell<media::Media> = const { OnceCell::new() };
    /// Where the taskbar was when a rebuild was requested: the rebuild timer waits until it stops moving.
    static REBUILD_TRAY: Cell<Option<(i32, i32, i32, i32)>> = const { Cell::new(None) };
    /// How many follow-up look captures after the window start are done.
    static BOOT_LOOK: Cell<usize> = const { Cell::new(0) };
    /// High half of a surrogate pair waiting for the low one.
    static HIGH_SURROGATE: Cell<Option<u16>> = const { Cell::new(None) };
    /// Notifications kept while the window is recreated: a pending "Claude is thinking" must not vanish.
    static ZONE_STASH: RefCell<Option<island::zone::Zone>> = const { RefCell::new(None) };
}

fn media_snapshot() -> media::State {
    MEDIA.with(|m| m.get().map(|m| m.snapshot()).unwrap_or_default())
}

fn media_send(cmd: media::Cmd) {
    MEDIA.with(|m| {
        if let Some(m) = m.get() {
            m.send(cmd);
        }
    });
}

fn wallpaper_changed() {
    let hwnd = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
    if !hwnd.is_invalid() {
        // SAFETY: posting a message has no memory-safety preconditions; a stale window only fails.
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_WALLPAPER, WPARAM(0), LPARAM(0));
        }
    }
}

/// Called from the media and weather threads: wakes the UI thread to redraw the island.
fn media_changed() {
    let hwnd = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
    if !hwnd.is_invalid() {
        // SAFETY: posting a message has no memory-safety preconditions; a stale window only fails.
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
        }
    }
}

/// Wakes the UI thread with `msg`, if the window exists.
fn wake(msg: u32) {
    let hwnd = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
    if !hwnd.is_invalid() {
        // SAFETY: posting a message has no memory-safety preconditions; a stale window only fails.
        unsafe {
            let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// Called from the pipe thread. The message waits in the queue even if there is no window right now.
fn push_arrived() {
    wake(WM_APP_PUSH);
}

/// Called from the system thread pool: a terminal command finished.
fn command_exited() {
    wake(WM_APP_TERM);
}

/// Runs `f` on the app state. A nested call (the window sends a message while the state is
/// already borrowed) does not panic but returns `None`.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn main() -> Result<()> {
    // With a command the exe is a client: it sends a message to the island and exits.
    if let Some(code) = cli::run() {
        std::process::exit(code);
    }
    if install::relocate(CLASS, ipc::INSTANCE_MUTEX) {
        return Ok(());
    }
    autostart::refresh();
    // SAFETY: runs on the main thread; every Win32 call gets handles and out pointers that stay valid
    // for its duration, and the message loop owns `msg`.
    unsafe {
        // A second instance on top of the first is not needed. On a relaunch from the settings the previous
        // instance is still exiting: wait for it, but no longer than 5 s.
        let relaunch = std::env::args().nth(1).as_deref() == Some("--relaunch");
        let mut _mutex = CreateMutexW(None, true, ipc::INSTANCE_MUTEX)?;
        let mut waited = 0;
        while GetLastError() == ERROR_ALREADY_EXISTS {
            if !relaunch || waited >= 50 {
                return Ok(());
            }
            let _ = windows::Win32::Foundation::CloseHandle(_mutex);
            std::thread::sleep(std::time::Duration::from_millis(100));
            waited += 1;
            _mutex = CreateMutexW(None, true, ipc::INSTANCE_MUTEX)?;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let cfg = config::load();
        i18n::init(cfg.language);
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });

        MEDIA.with(|m| {
            m.get_or_init(|| media::Media::start(media_changed));
        });
        audio::start();
        weather::start(cfg.city.clone(), media_changed);
        ipc::start(push_arrived);
        let off = modules::off(&cfg);
        modules::sync(&off);
        modules::init(push_arrived);
        modules::reconcile(&off, vertical());
        modules::start_packaged(&off);
        taskbar::watch(media_changed);
        wallpaper::watch(|| wallpaper().map(|w| w.0), wallpaper_changed);
        TERMINAL_MIN.store(cfg.terminal_notify, Ordering::Relaxed);
        terminal::init(command_exited);

        // A foreground change is the moment the taskbar rises above topmost windows.
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);

        // At autostart the taskbar may not exist yet; then wait for it.
        let mut retry = 0usize;
        if let Err(e) = spawn(instance, &cfg) {
            proto::error!("core", "create window: {e}");
            retry = SetTimer(None, 0, RESPAWN_RETRY_MS, None);
        }

        // Windowless thread messages signal a window rebuild; they are handled here, not in the window
        // procedure: there may be no window at that moment.
        let mut handle = |msg: &MSG| {
            let respawn = msg.message == WM_APP_RESPAWN || (msg.message == WM_TIMER && retry != 0 && msg.wParam.0 == retry);
            if msg.hwnd.is_invalid() && respawn {
                if spawn(instance, &cfg).is_ok() {
                    if retry != 0 {
                        let _ = KillTimer(None, retry);
                        retry = 0;
                    }
                } else if retry == 0 {
                    retry = SetTimer(None, 0, RESPAWN_RETRY_MS, None);
                }
                return;
            }
            let _ = TranslateMessage(msg);
            DispatchMessageW(msg);
        };

        // Frames run only during animation; at rest the thread sleeps in GetMessage.
        let mut msg = MSG::default();
        loop {
            if with_app(|a| a.animating).unwrap_or(false) {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return Ok(());
                    }
                    handle(&msg);
                }
                // Too early for a background frame: sleep until it, but wake on any input.
                let wait = with_app(|a| a.next_frame.map(|t| t.saturating_duration_since(Instant::now()))).flatten();
                if let Some(w) = wait.filter(|w| !w.is_zero()) {
                    MsgWaitForMultipleObjects(None, false, w.as_millis().max(1) as u32, QS_ALLINPUT);
                    continue;
                }
                with_app(|a| a.tick());
                continue;
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                return Ok(());
            }
            handle(&msg);
        }
    }
}

/// Creates the island window with the whole graphics stack and shows it.
///
/// The window is owned by the taskbar: in Windows an owned window always stays above its owner, so
/// the taskbar cannot cover the island when it raises itself. When Explorer restarts the owner is
/// reset while the window stays; the binding is restored on `TaskbarCreated`. If the window is
/// destroyed anyway, the main loop recreates it.
fn spawn(instance: HINSTANCE, cfg: &Config) -> Result<()> {
    // SAFETY: runs on the UI thread; every Win32 call gets handles and out pointers that stay valid
    // for its duration, and `hwnd` is the window just created.
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null())?;
        // Scale and size come from the taskbar's monitor, not the system: the system DPI is fixed at sign-in
        // and does not change until you sign in again.
        let monitor = MonitorFromWindow(tray, MONITOR_DEFAULTTONEAREST);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let dpi = dpi_x as f32;
        let scale = dpi / 96.0;
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let screen = info.rcMonitor;
        let h = taskbar::height(cfg.taskbar_height, scale);
        let bottom = taskbar::edge() == taskbar::Edge::Bottom;
        let bar_h = h;

        // The window spans the taskbar monitor's width: dropdowns flow out of any place. Only drawn parts
        // take the mouse (the window region); the rest is transparent.
        let (w_px, h_px) = (screen.right - screen.left, ((bar_h + PANEL_SPACE) * scale).round() as i32);
        let x = screen.left;
        let y = if bottom { screen.bottom - h_px } else { screen.top };
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("Polynya"),
            WS_POPUP,
            x,
            y,
            w_px,
            h_px,
            Some(tray),
            None,
            Some(instance),
            None,
        )?;
        HWND_ISLAND.store(hwnd.0 as isize, Ordering::Relaxed);

        let gfx = gfx::Gfx::new(hwnd, w_px as u32, h_px as u32, dpi)?;
        let light = is_light(cfg.theme);
        let theme = island::kit::Theme::from_config(cfg, light);
        let mut island = Island::new(&gfx.dwrite, h, w_px as f32 / scale, theme)?;
        island.set_frame(bottom, h_px as f32 / scale);
        // Dropdowns are made of glass (Mica).
        let glass = Glass::build(&gfx, screen, scale, if light { island::kit::LIGHT } else { island::kit::DARK }, (h, bottom));
        match &glass {
            Some(g) => {
                g.place((y - screen.top) as f32 / scale);
                island.set_glass(g.brush.clone(), g.near);
            }
            None => proto::warn!("look", "wallpaper not read, dropdowns without glass"),
        }
        let t = std::time::Instant::now();
        match gfx.warm() {
            Ok(()) => proto::info!("gfx", "effects warmed up in {} ms", t.elapsed().as_millis()),
            Err(e) => proto::warn!("gfx", "effect warm-up: {e}"),
        }
        if let Some(zone) = ZONE_STASH.with(|z| z.borrow_mut().take()) {
            island.set_zone(zone);
        }
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                hwnd,
                gfx,
                island,
                scale,
                left_px: x,
                bottom,
                monitor: screen,
                theme: cfg.theme,
                light,
                look: None,
                guard_at: None,
                glass,
                bar_h,
                taskbar_h: h,
                taskbar_override: cfg.taskbar_height,
                last: Instant::now(),
                animating: false,
                hook: None,
                minute: u16::MAX,
                taskbar_created,
                tracking_mouse: false,
                region: Vec::new(),
                broken: false,
                prev_focus: None,
                invoking: None,
                menu_restore: false,
                next_frame: None,
            })
        });
        let _ = windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(hwnd, windows::Win32::System::RemoteDesktop::NOTIFY_FOR_THIS_SESSION);
        // First capture right away, otherwise the island shows up in default colors.
        with_app(|a| a.capture_look());
        BOOT_LOOK.with(|b| b.set(0));
        SetTimer(Some(hwnd), TIMER_LOOK_BOOT, LOOK_BOOT_MS[0], None);
        SetTimer(Some(hwnd), TIMER_PREWARM, PREWARM_MS, None);

        add_tray_icon(hwnd);
        // Alt+Space may be taken by another launcher; then the magnifier click remains.
        if RegisterHotKey(Some(hwnd), HOTKEY_LAUNCHER, MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32).is_err() {
            proto::warn!("core", "Alt+Space is taken by another app");
        }
        sync_hotkeys(hwnd);
        a11y::init(hwnd, a11y::Messages { sync: WM_APP_A11Y_SYNC, action: WM_APP_A11Y_ACTION });
        // Started over a running game: do not show the window, the first check after exit shows it.
        if fullscreen_app_active(hwnd) || vertical() {
            HIDDEN.store(true, Ordering::Relaxed);
            audio::set_paused(true);
        } else {
            HIDDEN.store(false, Ordering::Relaxed);
            locks::listen(Some(hwnd));
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        with_app(|a| {
            // Messages that arrived while there was no window.
            a.apply_pushes();
            a.sync_note_timer();
            a.render();
        });
        SetTimer(Some(hwnd), TIMER_CLOCK, 1000, None);
        Ok(())
    }
}

/// Recreates the window if the taskbar height diverged from the one it was built for.
fn rebuild_if_taskbar_resized(hwnd: HWND) {
    // The taskbar changed height, moved to another edge or to another monitor: the island lives
    // where the primary taskbar is.
    let changed = with_app(|a| {
        (taskbar::height(a.taskbar_override, a.scale) - a.taskbar_h).abs() > 0.5
            || (taskbar::edge() == taskbar::Edge::Bottom) != a.bottom
            || taskbar_monitor() != Some(a.monitor)
    });
    if changed == Some(true) {
        proto::info!("core", "taskbar height, edge or monitor changed");
        request_rebuild(hwnd);
    }
}

thread_local! {
    /// The system look last written to the log.
    static LAST_LOOK: std::cell::RefCell<Option<(bool, look::Rgb, look::Rgb)>> = const { std::cell::RefCell::new(None) };
}

/// Light theme: set in the config or such a taskbar.
fn is_light(theme: config::Theme) -> bool {
    match theme {
        config::Theme::Os => taskbar::light(),
        t => t == config::Theme::Light,
    }
}

/// `WM_SETTINGCHANGE` about a color mode change: `lParam` is the string "ImmersiveColorSet".
fn is_color_set(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    // SAFETY: for this message `lParam` is either null (checked above) or a null-terminated string
    // valid while the message is handled.
    unsafe { PCWSTR(lparam.0 as *const u16).to_string() }.is_ok_and(|s| s == "ImmersiveColorSet")
}

/// The taskbar is vertical: the island does not live on it (no layout for it) and hides until the
/// taskbar returns to the top or bottom edge.
fn vertical() -> bool {
    taskbar::edge() == taskbar::Edge::Side
}

/// Desktop wallpaper: path and file modification time. Comparing them, the island notices a
/// wallpaper change, which Windows 11 does not always report.
fn wallpaper() -> Option<(String, std::time::SystemTime)> {
    let mut buf = [0u16; 512];
    // SAFETY: `buf` is writable for the length passed.
    unsafe {
        SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr().cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let path = String::from_utf16_lossy(&buf[..len]);
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    Some((path, modified))
}

/// Glass, like Mica: not the live screen but the desktop wallpaper, blurred once and laid out over
/// the screen as Windows shows it ("fill"). The brush lives in monitor coordinates; when the window
/// moves, so does the brush (`place`).
struct Glass {
    brush: windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush,
    /// Average glass color near the taskbar, where dropdowns grow.
    near: [f32; 3],
    /// Picture scale and offset relative to the monitor, DIP.
    k: f32,
    offset: (f32, f32),
    /// Which wallpaper went into the glass.
    source: (String, std::time::SystemTime),
}

impl Glass {
    /// `taskbar` is the taskbar height in DIPs and whether it is at the bottom: dropdowns grow at its edge.
    fn build(gfx: &gfx::Gfx, screen: RECT, scale: f32, palette: island::kit::Palette, taskbar: (f32, bool)) -> Option<Self> {
        use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_BRUSH_PROPERTIES1, D2D1_EXTEND_MODE_CLAMP, D2D1_INTERPOLATION_MODE_LINEAR};
        let source = wallpaper()?;
        let bytes = std::fs::read(&source.0).ok()?;
        // Small right away: a blur on a 1/8-screen picture looks the same at tens of times less memory
        // and work.
        let small_w = ((screen.right - screen.left) as f32 * GLASS_SCALE).round() as u32;
        let image = gfx.decode_scaled(&bytes, small_w).ok()?;
        let blurred = gfx.blurred(&image, GLASS_BLUR * GLASS_SCALE, 1.0).ok()?;
        drop(image);
        gfx.trim();
        // SAFETY: COM call on a live bitmap.
        let size = unsafe { blurred.GetSize() };
        let (mw, mh) = ((screen.right - screen.left) as f32 / scale, (screen.bottom - screen.top) as f32 / scale);
        let k = (mw / size.width).max(mh / size.height);
        let (iw, ih) = (size.width * k, size.height * k);
        let offset = ((mw - iw) / 2.0, (mh - ih) / 2.0);
        let (t, body) = (palette.glass_tint, palette.body);
        let tint = |avg: [f32; 3]| [avg[0] * (1.0 - t) + body.r * t, avg[1] * (1.0 - t) + body.g * t, avg[2] * (1.0 - t) + body.b * t];
        // The band at the taskbar edge where dropdowns grow.
        let (th, bottom) = taskbar;
        let (y0, y1) = if bottom { (mh - th - NEAR_BAND, mh - th) } else { (th, th + NEAR_BAND) };
        let near = tint(gfx.average(&bytes, ((0.0 - offset.0) / iw, (y0 - offset.1) / ih, (mw - offset.0) / iw, (y1 - offset.1) / ih)).ok()?);
        let props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
            extendModeX: D2D1_EXTEND_MODE_CLAMP,
            extendModeY: D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
        };
        // SAFETY: COM call on a live device context; `props` outlives the call.
        let brush = unsafe { gfx.dc.CreateBitmapBrush(&blurred, Some(&props), None).ok()? };
        Some(Self { brush: windows::core::Interface::cast(&brush).ok()?, near, k, offset, source })
    }

    /// Lays the picture out under a window whose top is `win_top` pixels below the monitor's top.
    fn place(&self, win_top: f32) {
        let m = windows_numerics::Matrix3x2 { M11: self.k, M12: 0.0, M21: 0.0, M22: self.k, M31: self.offset.0, M32: self.offset.1 - win_top };
        // SAFETY: COM call on a live brush.
        unsafe { self.brush.SetTransform(&m) };
    }
}

/// Rectangle of the primary taskbar's monitor.
fn taskbar_monitor() -> Option<RECT> {
    // SAFETY: `info` is initialized with its `cbSize`; the class name is a static string.
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()?;
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(MonitorFromWindow(tray, MONITOR_DEFAULTTONEAREST), &mut info).as_bool().then_some(info.rcMonitor)
    }
}

/// The system look may have changed: capture after a pause; new events extend the pause.
fn request_look(hwnd: HWND) {
    // SAFETY: `hwnd` is our window; a timer has no memory-safety preconditions.
    unsafe {
        SetTimer(Some(hwnd), TIMER_LOOK, LOOK_SETTLE_MS, None);
    }
}

/// Relaunch: a new instance with `--relaunch` waits for this one to exit and takes its place.
/// Interface language and the module set are read only at startup.
fn relaunch() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--relaunch").spawn();
    }
    QUITTING.store(true, Ordering::Relaxed);
    let hwnd = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
    // SAFETY: `hwnd` is our window on this thread; destroying a stale handle only fails.
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// IME mode of another process's window: whether it is open and the conversion mode. Its input
/// context cannot be read directly, so ask its IME window (`WM_IME_CONTROL`).
fn ime_state(hwnd: HWND) -> Option<(bool, u32)> {
    use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
    const IMC_GETCONVERSIONMODE: usize = 0x0001;
    const IMC_GETOPENSTATUS: usize = 0x0005;
    // SAFETY: `hwnd` may belong to another process; the IME queries only return integers.
    unsafe {
        let ime = ImmGetDefaultIMEWnd(hwnd);
        if ime.is_invalid() {
            return None;
        }
        let open = SendMessageW(ime, WM_IME_CONTROL, Some(WPARAM(IMC_GETOPENSTATUS)), Some(LPARAM(0))).0 != 0;
        let conversion = SendMessageW(ime, WM_IME_CONTROL, Some(WPARAM(IMC_GETCONVERSIONMODE)), Some(LPARAM(0))).0 as u32;
        Some((open, conversion))
    }
}

/// Rectangle of the primary taskbar, screen pixels.
fn tray_rect() -> Option<(i32, i32, i32, i32)> {
    // SAFETY: `r` is a valid out pointer; the class name is a static string.
    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()?;
        let mut r = RECT::default();
        GetWindowRect(tray, &mut r).ok()?;
        Some((r.left, r.top, r.right, r.bottom))
    }
}

/// Recreates the island window with the whole graphics stack after a short pause.
///
/// One path for every case where what was built for old conditions no longer fits: a lost
/// video device, a scale or resolution change. The window is destroyed not via "Exit", so the
/// main loop recreates it for the current conditions; the media, audio and weather threads are
/// not restarted.
fn request_rebuild(hwnd: HWND) {
    REBUILD_TRAY.with(|r| r.set(tray_rect()));
    // SAFETY: `hwnd` is our window; a timer has no memory-safety preconditions.
    unsafe {
        SetTimer(Some(hwnd), TIMER_REBUILD, REBUILD_DEBOUNCE_MS, None);
    }
}

/// Makes the window owned by the current taskbar and raises it above it.
fn attach_to_taskbar(hwnd: HWND) {
    // SAFETY: `hwnd` is our window; setting its owner to the taskbar handle is a plain Win32 call.
    unsafe {
        if let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, tray.0 as isize);
        }
    }
    raise();
}

/// Returns the island to the top of the topmost layer.
fn raise() {
    let hwnd = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
    if !hwnd.is_invalid() && !HIDDEN.load(Ordering::Relaxed) {
        // SAFETY: `hwnd` is our window; repositioning has no memory-safety preconditions.
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    let hwnd = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
    if !hwnd.is_invalid() {
        // The fullscreen check touches the window, so do it from the window procedure, not the hook.
        // SAFETY: posting a message has no memory-safety preconditions; a stale window only fails.
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_FOREGROUND, WPARAM(0), LPARAM(0));
        }
    }
    raise();
}

/// Process of the foreground window; our own window does not count.
fn foreground_pid() -> u32 {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    // SAFETY: `GetCurrentProcessId` has no preconditions.
    if pid == unsafe { GetCurrentProcessId() } { 0 } else { pid }
}

/// The foreground changed: the terminal the user left gets its commands watched.
fn track_terminal() {
    if TERMINAL_MIN.load(Ordering::Relaxed) == 0 {
        return;
    }
    let to = foreground_pid();
    if to == 0 {
        return;
    }
    let from = LAST_FOREGROUND.swap(to, Ordering::Relaxed);
    if from != to {
        terminal::foreground_changed(from, to);
    }
}

/// Not the time for popups, as Windows decides for its own banners: Do Not Disturb (notification
/// mode is not `Unrestricted`), a presentation or a fullscreen app; or the island is hidden.
///
/// In Do Not Disturb Windows lets priority apps through, but their list is not published, so the
/// island stays quiet for everyone.
fn quiet() -> bool {
    use windows::UI::Notifications::{ToastNotificationManager, ToastNotificationMode};
    let dnd = ToastNotificationManager::GetDefault().and_then(|m| m.NotificationMode()).is_ok_and(|m| m != ToastNotificationMode::Unrestricted);
    // SAFETY: `SHQueryUserNotificationState` has no preconditions.
    HIDDEN.load(Ordering::Relaxed) || dnd || unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s != QUNS_ACCEPTS_NOTIFICATIONS)
}

/// Whether the foreground window covers the island's whole monitor (a game, fullscreen video, a
/// presentation).
///
/// A maximized window does not count: it does not cover the taskbar.
fn fullscreen_app_active(island: HWND) -> bool {
    // SAFETY: window handles may be stale, which only makes calls fail; `class`, `info` and `rect`
    // are valid out buffers.
    unsafe {
        // The system reports exclusive D3D mode and presentations itself.
        if let Ok(state) = SHQueryUserNotificationState()
            && (state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE)
        {
            return true;
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == island {
            return false;
        }
        // The desktop and the taskbar itself cover the screen but are not fullscreen apps.
        let mut class = [0u16; 32];
        let len = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        // The island lives on one monitor; a fullscreen window on another does not bother it.
        if monitor != MonitorFromWindow(island, MONITOR_DEFAULTTONEAREST) {
            return false;
        }
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let mut rect = RECT::default();
        if !GetMonitorInfoW(monitor, &mut info).as_bool() || GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let m = info.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

impl App {
    /// The glass follows the window: with a bottom taskbar the window grows upward and its top moves.
    fn place_glass(&self) {
        let Some(g) = &self.glass else { return };
        let mut r = RECT::default();
        // SAFETY: `r` is a valid out pointer for our own window.
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        g.place((r.top - self.monitor.top) as f32 / self.scale);
    }

    /// Captures the system look: the taskbar strip under the island, the theme and the accent.
    fn capture_look(&mut self) {
        // SAFETY: the class name is a static string.
        let Ok(tray) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else { return };
        let mut r = RECT::default();
        // SAFETY: `r` is a valid out pointer.
        if unsafe { GetWindowRect(tray, &mut r) }.is_err() {
            return;
        }
        let t = Instant::now();
        let Some(look) = look::Look::capture(self.hwnd, r, self.bottom) else {
            return proto::warn!("look", "taskbar strip not captured");
        };
        let l = (self.left_px - r.left).max(0) as usize;
        proto::debug!(
            "look",
            "captured in {} ms: {}, accent {}, taskbar {} (edge {}), grain {:.3}",
            t.elapsed().as_millis(),
            if look.light { "light" } else { "dark" },
            look::hex(look.accent),
            look::hex(look.mean_over(l, l + 200)),
            look::hex(look.edge.get(l + 100).copied().unwrap_or_default()),
            look.noise,
        );
        // Text follows the theme (the config toggle is only for it); plates follow the taskbar
        // under the island.
        let light = match self.theme {
            config::Theme::Os => look.light,
            t => t == config::Theme::Light,
        };
        let bg = match self.island.bar_span() {
            Some((sl, sr)) => {
                let x = |v: f32| ((v * self.scale) as i32 + self.left_px - r.left).max(0) as usize;
                look.mean_over(x(sl), x(sr))
            }
            None => look.mean_over(l, l + 200),
        };
        self.light = light;
        let edge_x = (r.left - self.left_px) as f32 / self.scale;
        self.island.set_look(light, bg, &look, edge_x);
        proto::debug!("look", "plates: taskbar {}, {}", look::hex(bg), self.island.plate_info());
        // Captures are frequent (start, wallpaper, theme, unlock), so the regular log gets only look changes.
        // A recapture differs by a unit here and there; only a visible change is logged.
        let near = |a: look::Rgb, b: look::Rgb| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 2.0 / 255.0);
        let now = (light, look.accent, bg);
        let changed = LAST_LOOK.with(|l| l.replace(Some(now))).is_none_or(|(pl, pa, pb)| pl != light || !near(pa, now.1) || !near(pb, bg));
        if changed {
            proto::info!("look", "{}, accent {}, taskbar {}", if light { "light" } else { "dark" }, look::hex(look.accent), look::hex(bg));
        }
        self.look = Some(look);
        self.refresh();
    }

    /// The wallpaper changed: the glass is rebuilt without recreating the window.
    fn check_wallpaper(&mut self) {
        if self.glass.as_ref().map(|g| &g.source) == wallpaper().as_ref() {
            return;
        }
        let Some(g) = Glass::build(&self.gfx, self.monitor, self.scale, self.island.palette(), (self.taskbar_h, self.bottom)) else { return };
        self.island.set_glass(g.brush.clone(), g.near);
        self.glass = Some(g);
        self.place_glass();
        self.refresh();
    }

    /// The window under dropdowns grows when a panel taller than it opens: the content sets the height,
    /// not a constant. It never shrinks back until the next window rebuild.
    fn fit_window(&mut self) {
        let need = self.bar_h + self.island.wanted_depth() * GROW_OVERSHOOT + GROW_PAD;
        if need * self.scale <= self.gfx.height() as f32 {
            return;
        }
        let dip = (need / GROW_STEP).ceil() * GROW_STEP;
        let px = (dip * self.scale).round() as i32;
        // SAFETY: `self.hwnd` is our window on this thread; `r` is a valid out pointer.
        unsafe {
            let mut r = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut r);
            // With a bottom taskbar the bottom edge stays put and the window grows upward.
            let top = if self.bottom { r.bottom - px } else { r.top };
            let _ = SetWindowPos(self.hwnd, None, r.left, top, r.right - r.left, px, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        if let Err(e) = self.gfx.grow(px as u32) {
            proto::error!("core", "grow window: {e}");
        }
        self.place_glass();
        self.island.set_frame(self.bottom, px as f32 / self.scale);
    }

    fn render(&mut self) {
        if self.broken {
            return;
        }
        self.fit_window();
        let free = taskbar::free().map(|(l, r)| ((l - self.left_px) as f32 / self.scale, (r - self.left_px) as f32 / self.scale));
        self.island.set_free(free);
        let snap = media_snapshot();
        let wx = weather::current();
        let drawn = {
            let (island, gfx) = (&mut self.island, &self.gfx);
            gfx.frame(|dc| {
                if let Err(e) = island.draw(gfx, dc, &snap, wx.as_ref()) {
                    proto::error!("gfx", "draw: {e}");
                }
            })
        };
        // The region goes before the frame is shown and with its own size: set afterwards, it lagged a
        // frame and clipped the growing dropdown (on expand and on spring overshoot).
        if drawn.is_ok() {
            self.update_region();
            if a11y::active() {
                a11y::stage(self.hwnd, self.island.access(), self.scale);
            }
        }
        let (island, gfx) = (&mut self.island, &self.gfx);
        let framed = drawn.and_then(|_| {
            // Layers are drawn after the frame: two composition surfaces cannot be drawn at once.
            if let Err(e) = island.draw_layers(gfx) {
                proto::error!("gfx", "layers: {e}");
            }
            gfx.present()
        });
        if let Err(e) = framed {
            proto::error!("gfx", "frame: {e}");
            if gfx::Gfx::is_device_lost(&e) {
                // Device lost (sleep, driver): the whole stack is recreated together with the window.
                self.broken = true;
                self.animating = false;
                request_rebuild(self.hwnd);
                return;
            }
        }
        // A component may have noticed a change while drawing (the minute changed, time to roll the
        // digits) and asked for continuous frames.
        if !self.animating && self.island.wants_frames() {
            self.animate();
        }
        if !self.animating {
            self.sync_wake_timer();
        }
    }

    /// Wakes the island for the next picture change that needs no continuous frames.
    fn sync_wake_timer(&self) {
        // SAFETY: `self.hwnd` is our window on this thread; the Win32 calls take it and plain values.
        unsafe {
            match self.island.next_wake() {
                Some(s) => {
                    SetTimer(Some(self.hwnd), TIMER_WAKE, ((s * 1000.0).ceil() as u32).max(1), None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_WAKE);
                }
            }
        }
    }

    /// Applies messages from the pipe.
    fn apply_pushes(&mut self) {
        self.island.set_quiet(quiet());
        for p in ipc::drain() {
            match p {
                ipc::Push::Notify(note) => self.island.notify(note),
                ipc::Push::Dismiss(key) => {
                    if self.island.dismiss_key(&key) {
                        self.close();
                    }
                }
            }
        }
        for (module, mut msg) in modules::drain() {
            if let proto::ModOut::Open { screen, .. } | proto::ModOut::Push { screen } | proto::ModOut::Update { screen } = &mut msg {
                hotkeys::fill(&module, screen);
            }
            match msg {
                proto::ModOut::Open { screen, at } => self.module_open(module, screen, at),
                proto::ModOut::Push { screen } if self.island.menu_module() == Some(module.as_str()) => {
                    self.island.menu_show(module, screen, None);
                }
                proto::ModOut::Push { screen } => self.module_open(module, screen, None),
                proto::ModOut::Update { screen } => {
                    if self.island.menu_module() == Some(module.as_str()) {
                        self.island.menu_replace(&module, screen);
                    } else {
                        proto::debug!("modules", "{module}: update while its dropdown is closed, ignored");
                    }
                }
                proto::ModOut::Close => {
                    if self.island.menu_module() == Some(module.as_str()) {
                        self.close_menu(self.menu_restore);
                    } else if self.invoking.as_deref() == Some(module.as_str()) {
                        // The command did its work without a screen of its own.
                        self.invoking = None;
                        self.close_launcher(true);
                    }
                }
                proto::ModOut::Copy { text } => launcher::copy(&text),
            }
        }
        self.sync_note_timer();
        self.animate();
    }

    /// Opens a module's dropdown with `screen`. Without `at` it hangs under the search block; right
    /// after the module's launcher command it replaces the launcher as a page of search.
    fn module_open(&mut self, module: String, screen: proto::Screen, at: Option<[i32; 2]>) {
        let invoked = self.invoking.as_deref() == Some(module.as_str());
        let from_launcher = invoked && self.island.launcher_open();
        self.invoking = None;
        if self.island.menu_module() == Some(module.as_str()) {
            self.island.menu_restart(&module, screen);
            self.animate();
            return;
        }
        // No island on a vertical taskbar, and so no search block to hang under.
        if vertical() && at.is_none() {
            proto::warn!("modules", "{module}: open without at on a vertical taskbar, ignored");
            return;
        }
        if HIDDEN.load(Ordering::Relaxed) && !vertical() {
            self.unhide();
        }
        let span = match at {
            Some([l, r]) => ((l - self.left_px) as f32 / self.scale, (r - l) as f32 / self.scale),
            None => self.island.search_span(),
        };
        if from_launcher {
            // The launcher already has focus; swap it for the page like the settings do.
            self.island.close();
            self.island.menu_open_as(module, screen, span, true, true);
        } else {
            self.menu_closed();
            self.open_menu();
            // Under the search block it is a tool, not a taskbar menu: give it room.
            self.island.menu_open_as(module, screen, span, false, at.is_none());
        }
        self.menu_restore = invoked;
        self.sync_hook();
        self.animate();
    }

    /// The window takes focus for the launcher or a module dropdown: they need the keyboard, while the
    /// island never takes focus on principle. The window that had focus is remembered.
    fn take_focus(&mut self) {
        // SAFETY: `self.hwnd` is our window on this thread; the IME context is released after use.
        unsafe {
            let fg = GetForegroundWindow();
            self.prev_focus = (fg != self.hwnd && !fg.is_invalid()).then_some(fg);
            let ime = self.prev_focus.and_then(ime_state);
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
            let _ = SetForegroundWindow(self.hwnd);
            // IME mode as in the window we came from: the island has its own input context, and without this
            // CJK input reset to Latin on every open.
            if let Some((open, conversion)) = ime {
                use windows::Win32::UI::Input::Ime::{
                    IME_CONVERSION_MODE, IME_SENTENCE_MODE, ImmGetContext, ImmReleaseContext, ImmSetConversionStatus, ImmSetOpenStatus,
                };
                let imc = ImmGetContext(self.hwnd);
                if !imc.is_invalid() {
                    let _ = ImmSetOpenStatus(imc, open);
                    if open {
                        let _ = ImmSetConversionStatus(imc, IME_CONVERSION_MODE(conversion), IME_SENTENCE_MODE(0));
                    }
                    let _ = ImmReleaseContext(self.hwnd, imc);
                }
            }
        }
    }

    /// Gives focus back; `restore` returns it to the window that had it.
    fn release_focus(&mut self, restore: bool) {
        // SAFETY: `self.hwnd` is our window on this thread; the Win32 calls take it and plain values.
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
            // Only from the island itself: the user may have switched to another window meanwhile.
            if let Some(prev) = self.prev_focus.take().filter(|_| restore && GetForegroundWindow() == self.hwnd) {
                let _ = SetForegroundWindow(prev);
            }
        }
    }

    /// A module dropdown opens: focus goes to the island, the launcher closes.
    fn open_menu(&mut self) {
        if self.island.launcher_open() {
            self.close_launcher(false);
        }
        if !self.island.menu_open() {
            self.take_focus();
        }
    }

    fn close_menu(&mut self, restore: bool) {
        if !self.island.menu_open() || self.settings_guard() {
            return;
        }
        self.menu_closed();
        self.island.close();
        self.sync_hook();
        self.animate();
        self.release_focus(restore);
        // The catalog is stale: reread it now, not on the next open.
        launcher::apps::refresh(media_changed);
    }

    /// Opens the settings as a search page. `false`: the island is hidden (vertical taskbar),
    /// nowhere to show them.
    fn open_settings(&mut self) -> bool {
        if vertical() {
            return false;
        }
        if HIDDEN.load(Ordering::Relaxed) {
            self.unhide();
        }
        if self.island.menu_module() == Some(settings::MODULE) {
            return true;
        }
        self.open_menu();
        self.show_settings();
        true
    }

    /// Settings dropdown from the search block; the island already has focus.
    fn show_settings(&mut self) {
        let at = self.island.search_span();
        self.island.menu_open_as(settings::MODULE.into(), settings::main(), at, true, true);
        self.sync_hook();
        self.animate();
    }

    /// Guards against closing the settings with unapplied changes: instead of closing, a warning
    /// screen. If it is already open, closing discards the changes. `true`: the close was intercepted.
    fn settings_guard(&mut self) -> bool {
        if self.island.menu_module() != Some(settings::MODULE) || !settings::pending() {
            return false;
        }
        if settings::is_guard(self.island.menu_title(settings::MODULE)) {
            // A click outside gives two closes in a row (click interception and activation loss): the second
            // must not immediately discard the warning just shown.
            if self.guard_at.is_some_and(|t| t.elapsed() < GUARD_GRACE) {
                return true;
            }
            settings::discard();
            return false;
        }
        self.island.menu_show(settings::MODULE.into(), settings::guard(), None);
        self.guard_at = Some(Instant::now());
        self.animate();
        true
    }

    /// A settings item was selected or text was entered in a field.
    fn settings(&mut self, id: &str, text: Option<String>) {
        let (act, change) = settings::select(id, text);
        match change {
            Some(settings::Change::Theme(t)) => {
                self.theme = t;
                self.capture_look();
            }
            Some(settings::Change::City(c)) => weather::set_city(c),
            Some(settings::Change::Terminal(s)) => TERMINAL_MIN.store(s, Ordering::Relaxed),
            Some(settings::Change::Module(id)) => {
                modules::toggled(&id, &modules::off(&config::load()), vertical());
                sync_hotkeys(self.hwnd);
            }
            None => {}
        }
        match act {
            settings::Act::Replace { screen, root } => {
                self.island.menu_replace(settings::MODULE, screen);
                if let Some(root) = root {
                    self.island.menu_replace_root(settings::MODULE, root);
                }
            }
            settings::Act::Push(screen) => self.island.menu_show(settings::MODULE.into(), screen, None),
            settings::Act::Restart => {
                relaunch();
                return;
            }
            settings::Act::Close => {
                self.close_menu(true);
                return;
            }
            settings::Act::None => {}
        }
        self.animate();
    }

    /// Tells the module its dropdown closed: it may care whether it is open.
    fn menu_closed(&self) {
        if let Some(m) = self.island.menu_module().filter(|m| *m != settings::MODULE) {
            modules::send(m, &proto::ModIn::Closed);
        }
    }

    fn menu_effect(&mut self, e: island::MenuEffect) {
        use island::MenuEffect;
        match e {
            MenuEffect::None => {}
            MenuEffect::Redraw => self.animate(),
            MenuEffect::Close => self.close_menu(true),
            // Settings are a search page: "back" from the first screen returns to search.
            MenuEffect::Leave => {
                if self.settings_guard() {
                    return;
                }
                self.menu_closed();
                self.island.close();
                launcher::apps::ensure(media_changed);
                self.island.open_launcher();
                self.sync_hook();
                self.animate();
            }
            MenuEffect::Select(id) if self.island.menu_module() == Some(settings::MODULE) => self.settings(&id, None),
            MenuEffect::Submit(id, text) if self.island.menu_module() == Some(settings::MODULE) => self.settings(&id, Some(text)),
            MenuEffect::Select(id) => {
                if let Some(m) = self.island.menu_module().map(String::from) {
                    modules::send(&m, &proto::ModIn::Select { id });
                }
                self.animate();
            }
            MenuEffect::Submit(id, text) => {
                if let Some(m) = self.island.menu_module().map(String::from) {
                    modules::send(&m, &proto::ModIn::Submit { id, text });
                }
                self.animate();
            }
            MenuEffect::Run(command) => {
                if let Some(m) = self.island.menu_module().map(String::from)
                    && let Err(e) = modules::invoke(&m, &command, proto::Source::Menu, "", String::new())
                {
                    proto::error!("modules", "{m}: {command}: {e}");
                }
                self.animate();
            }
        }
    }

    /// Redraw on an external event. If the island needs continuous frames (visualizer, marquee), starts
    /// the frame loop instead of a one-off draw.
    fn refresh(&mut self) {
        if self.animating || HIDDEN.load(Ordering::Relaxed) {
            return;
        }
        // The Everything reply or the app catalog arrives when the launcher is already at rest: no animation
        // step runs, and without this the results would never be recomputed.
        if self.island.sync_launcher() || self.island.wants_frames() {
            self.animate();
        } else {
            self.render();
        }
    }

    fn tick(&mut self) {
        // Hiding may have arrived mid frame loop; a hidden island does not draw.
        if HIDDEN.load(Ordering::Relaxed) {
            self.animating = false;
            return;
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        let moving = self.island.step(dt);
        // A notification may have expanded by itself; a click outside must close it.
        self.sync_hook();
        self.animating = moving || self.island.wants_frames();
        // Hover and expansion pause and rearm notification deadlines.
        if !self.animating {
            self.sync_note_timer();
        }
        // Animations run at the monitor rate; background frames less often, on a timer: motion there is
        // slow, and every frame costs CPU.
        self.next_frame = (!moving && self.animating).then(|| now + BACKGROUND_FRAME);
        self.render();
    }

    fn animate(&mut self) {
        // An event (hover, click) responds at once, without waiting for the background frame.
        self.next_frame = None;
        if !self.animating {
            self.last = Instant::now();
            self.animating = true;
        }
    }

    /// Sets the timer for the nearest notification deadline, or removes it if there are none.
    fn sync_note_timer(&self) {
        // SAFETY: `self.hwnd` is our window on this thread; the Win32 calls take it and plain values.
        unsafe {
            match self.island.next_note_deadline() {
                Some(at) => {
                    let ms = at.saturating_duration_since(Instant::now()).as_millis().clamp(10, u32::MAX as u128) as u32;
                    SetTimer(Some(self.hwnd), TIMER_NOTES, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_NOTES);
                }
            }
        }
    }

    fn notify(&mut self, note: island::zone::Note) {
        self.island.notify(note);
        self.sync_note_timer();
        self.animate();
    }

    /// Only the pill, chips and the flow take the mouse; the rest of the window is click-through.
    ///
    /// The region is set only on change: every set makes the system send a synthetic `WM_MOUSEMOVE`,
    /// which during animation is a flood of messages every frame.
    fn update_region(&mut self) {
        let s = self.scale;
        let rects: Vec<(i32, i32, i32, i32)> = self
            .island
            .regions()
            .into_iter()
            .map(|(l, t, r, b)| ((l * s).floor() as i32, (t * s).floor() as i32, (r * s).ceil() as i32, (b * s).ceil() as i32))
            .collect();
        if rects == self.region {
            return;
        }
        // SAFETY: every region is created here and either deleted once or handed to the system with
        // `SetWindowRgn`, which takes ownership.
        unsafe {
            let mut acc = None;
            for &(l, t, r, b) in &rects {
                let rgn = CreateRectRgn(l, t, r, b);
                match acc {
                    None => acc = Some(rgn),
                    Some(a) => {
                        CombineRgn(Some(a), Some(a), Some(rgn), RGN_OR);
                        let _ = DeleteObject(rgn.into());
                    }
                }
            }
            // After SetWindowRgn the region belongs to the system and must not be deleted.
            if let Some(rgn) = acc {
                SetWindowRgn(self.hwnd, Some(rgn), false);
            }
        }
        self.region = rects;
    }

    fn open(&mut self, panel: Panel, anchor: Part) {
        if panel == Panel::Launcher {
            return self.toggle_launcher();
        }
        if self.island.launcher_open() {
            self.close_launcher(false);
        }
        if self.island.menu_open() {
            self.close_menu(false);
        }
        self.island.toggle(panel, anchor);
        self.sync_hook();
        self.animate();
    }

    fn close(&mut self) {
        if self.island.launcher_open() {
            return self.close_launcher(false);
        }
        if self.island.menu_open() {
            return self.close_menu(false);
        }
        if self.island.open.is_some() {
            self.island.close();
            self.sync_hook();
            self.animate();
        }
    }

    /// The launcher needs keyboard input, while the island never takes focus on principle. While the
    /// launcher is open the window becomes active, and everything is restored on close. It opens by
    /// hotkey or click, and in both cases the system allows taking focus: this process just got input.
    fn toggle_launcher(&mut self) {
        if self.island.launcher_open() {
            return self.close_launcher(true);
        }
        if self.island.menu_open() {
            if self.settings_guard() {
                return;
            }
            self.menu_closed();
            self.island.close();
        } else {
            self.take_focus();
        }
        // A module dropped into the folder or fixed since the last open starts now.
        modules::reconcile(&modules::off(&config::load()), vertical());
        sync_hotkeys(self.hwnd);
        launcher::apps::ensure(media_changed);
        launcher::money::refresh(media_changed);
        self.island.open_launcher();
        self.sync_hook();
        self.animate();
    }

    /// Closes the launcher; `restore` returns focus to the window it was opened from.
    fn close_launcher(&mut self, restore: bool) {
        if !self.island.launcher_open() {
            return;
        }
        self.invoking = None;
        self.island.close();
        self.sync_hook();
        self.animate();
        self.release_focus(restore);
    }

    /// Runs a module command from its hotkey; `selection` is what was selected in the active app.
    /// Like from search, the module answers with a screen, or does its work and closes (the timer
    /// only stops waiting for it).
    fn hotkey_invoke(&mut self, b: hotkeys::Bound, selection: String) {
        match modules::invoke(&b.module, &b.command, proto::Source::Hotkey, "", selection) {
            Ok(()) => {
                self.invoking = Some(b.module);
                // SAFETY: `self.hwnd` is our window; a timer has no memory-safety preconditions.
                unsafe { SetTimer(Some(self.hwnd), TIMER_INVOKE, INVOKE_WAIT_MS, None) };
            }
            Err(e) => proto::error!("modules", "{}: hotkey: {e}", b.module),
        }
    }

    fn launcher_effect(&mut self, e: launcher::Effect) {
        match e {
            launcher::Effect::None => {}
            launcher::Effect::Redraw => self.animate(),
            launcher::Effect::Close { restore } => self.close_launcher(restore),
            launcher::Effect::Launch(id) => {
                // The launched app takes focus itself.
                self.close_launcher(false);
                launcher::apps::launch(&id);
            }
            // Settings are a search page: the same dropdown, focus stays with the island.
            launcher::Effect::Invoke(target) if target == format!("\t{}", settings::MODULE) => {
                self.island.close();
                self.show_settings();
            }
            launcher::Effect::Invoke(target) => {
                let Some((id, cmd)) = target.split_once('\t') else { return };
                // The launcher stays open until the module answers: `open` turns it into the module's
                // page, `close` or silence closes it.
                match modules::invoke(id, cmd, proto::Source::Search, &self.island.launcher_query(), String::new()) {
                    Ok(()) => {
                        self.invoking = Some(id.to_string());
                        // SAFETY: `self.hwnd` is our window; a timer has no memory-safety preconditions.
                        unsafe { SetTimer(Some(self.hwnd), TIMER_INVOKE, INVOKE_WAIT_MS, None) };
                    }
                    Err(e) => {
                        proto::error!("modules", "{id}: invoke: {e}");
                        self.close_launcher(true);
                    }
                }
            }
            launcher::Effect::Admin(path) => {
                self.close_launcher(false);
                launcher::apps::run_as(path);
            }
            launcher::Effect::Reveal(path) => {
                self.close_launcher(false);
                launcher::apps::reveal(path);
            }
            launcher::Effect::Open(target) => {
                self.close_launcher(false);
                launcher::apps::open(target);
            }
            launcher::Effect::Copy(text) => {
                launcher::copy(&text);
                self.close_launcher(true);
            }
        }
    }

    /// Hides the island under a fullscreen app and brings it back afterwards.
    ///
    /// A hidden island draws nothing and does not analyze audio: background frames are CPU taken from
    /// the game.
    fn sync_fullscreen(&mut self) {
        // A vertical taskbar has no room for the island; it hides just like under a game.
        let fullscreen = fullscreen_app_active(self.hwnd) || vertical();
        if fullscreen == HIDDEN.load(Ordering::Relaxed) {
            return;
        }
        HIDDEN.store(fullscreen, Ordering::Relaxed);
        audio::set_paused(fullscreen);
        // SAFETY: `self.hwnd` is our window on this thread; the Win32 calls take it and plain values.
        unsafe {
            if fullscreen {
                // An open panel must not wait for the return: collapse it without animation.
                self.island.close_now();
                self.sync_hook();
                self.animating = false;
                // In a game the island is invisible; it does not need key presses.
                locks::listen(None);
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            } else {
                self.unhide();
            }
        }
    }

    /// Shows the hidden island. Also called bypassing the fullscreen check, when the launcher or Start
    /// is invoked over a game.
    fn unhide(&mut self) {
        HIDDEN.store(false, Ordering::Relaxed);
        audio::set_paused(false);
        locks::listen(Some(self.hwnd));
        // SAFETY: `self.hwnd` is our window on this thread; the Win32 calls take it and plain values.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        raise();
        self.refresh();
    }

    /// Places the IME composition window at the launcher caret.
    fn place_ime(&self) {
        use windows::Win32::UI::Input::Ime::{
            CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow,
        };
        let Some((x, top, bottom)) = self.island.launcher_caret() else { return };
        let s = self.scale;
        let pt = POINT { x: (x * s) as i32, y: (top * s) as i32 };
        // SAFETY: the IME context of our window is released after use; the forms outlive the calls.
        unsafe {
            let imc = ImmGetContext(self.hwnd);
            if imc.is_invalid() {
                return;
            }
            let _ = ImmSetCompositionWindow(imc, &COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: pt, ..Default::default() });
            // Candidates go below the line, without covering it.
            let area = RECT { left: pt.x, top: pt.y, right: pt.x + 1, bottom: (bottom * s) as i32 };
            let _ =
                ImmSetCandidateWindow(imc, &CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: POINT { x: pt.x, y: area.bottom }, rcArea: area });
            let _ = ImmReleaseContext(self.hwnd, imc);
        }
    }

    /// The mouse hook is needed only while a panel is open, to close it with a click outside the island.
    fn sync_hook(&mut self) {
        match (self.island.open.is_some(), self.hook) {
            (true, None) => self.hook = start_mouse_hook(),
            (false, Some(tid)) => {
                stop_mouse_hook(tid);
                self.hook = None;
            }
            _ => {}
        }
    }

    /// A click at a window point in DIPs: from the mouse, or from a screen reader pressing a control.
    fn click_at(&mut self, x: f32, y: f32) {
        self.island.hide_tip();
        match self.island.click(x, y) {
            Action::NotificationCenter => {
                self.close();
                open_notification_center();
            }
            Action::Open(panel, part) => self.open(panel, part),
            Action::Media(cmd) => media_send(cmd),
            Action::Launch(i) => {
                let e = self.island.launcher_click(i);
                self.launcher_effect(e);
            }
            Action::LaunchAct(j) => {
                let e = self.island.launcher_act(j);
                self.launcher_effect(e);
            }
            Action::Dismiss => {
                self.island.dismiss_open();
                self.close();
            }
            Action::MenuRow(i) => {
                let e = self.island.menu_click(i);
                self.menu_effect(e);
            }
            Action::MenuBack => {
                let e = self.island.menu_back();
                self.menu_effect(e);
            }
            Action::OpenApp(part) => {
                if let Some(app) = self.island.note_app(part) {
                    launcher::apps::launch(&app);
                }
                self.island.dismiss_note(part);
                self.close();
            }
            Action::Swatch(i) => {
                if let Some(hex) = self.island.pick_swatch(i) {
                    launcher::copy(&hex);
                    self.animate();
                }
            }
            Action::Close => self.close(),
            Action::None => {}
        }
    }

    fn to_dip(&self, lparam: LPARAM) -> (f32, f32) {
        let x = (lparam.0 & 0xffff) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
        (x / self.scale, y / self.scale)
    }
}

/// Starts the low-level mouse hook in a separate thread. Returns the thread id.
///
/// The system holds every mouse event until the hook thread handles it. On the UI thread the hook
/// would wait for the frame to end (Present blocks until vblank), and the cursor across the whole
/// system would lag behind the mouse. The hook thread does nothing but the hook.
fn start_mouse_hook() -> Option<u32> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("mouse-hook".into())
        // SAFETY: the hook is installed, served and removed on this thread; `msg` outlives the loop.
        .spawn(move || unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
            let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(instance), 0) else {
                let _ = tx.send(None);
                return;
            };
            let _ = tx.send(Some(GetCurrentThreadId()));
            // The hook needs a message loop in its thread; WM_QUIT from stop_mouse_hook ends it.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            let _ = UnhookWindowsHookEx(hook);
        })
        .ok()?;
    rx.recv().ok().flatten()
}

fn stop_mouse_hook(tid: u32) {
    // SAFETY: posting to a thread id has no memory-safety preconditions; a finished thread only fails.
    unsafe {
        let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: for `code >= 0` the system passes a valid `MSLLHOOKSTRUCT` in `lparam` for this call.
    unsafe {
        if code >= 0 && matches!(wparam.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let ours = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
            // WindowFromPoint honors the window region: a click in the transparent part does not land here.
            if WindowFromPoint(info.pt) != ours {
                let _ = PostMessageW(Some(ours), WM_APP_OUTSIDE, WPARAM(0), LPARAM(0));
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by the system on the UI thread with valid arguments; message parameters are used
    // only as the documented types for each message, and handles passed on are our own window's.
    unsafe {
        match msg {
            // A click on the island must not steal focus from the current window.
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            // A press in a text field places the caret and starts a selection; the mouse is captured so
            // a drag past the field keeps selecting.
            WM_LBUTTONDOWN => {
                let took = with_app(|a| {
                    let (x, y) = a.to_dip(lparam);
                    let took = a.island.press(x, y, GetKeyState(VK_SHIFT.0 as i32) < 0);
                    if took {
                        a.animate();
                    }
                    took
                });
                if took == Some(true) {
                    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP => {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                with_app(|a| {
                    // The end of a press in a field is not a click.
                    if a.island.release() {
                        return;
                    }
                    let (x, y) = a.to_dip(lparam);
                    a.click_at(x, y);
                });
                return LRESULT(0);
            }
            WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
                let over = with_app(|a| {
                    let (x, y) = a.to_dip(LPARAM(((p.y as u16 as isize) << 16) | p.x as u16 as isize));
                    a.island.over_field(x, y)
                });
                if over == Some(true) {
                    if let Ok(c) = LoadCursorW(None, IDC_IBEAM) {
                        SetCursor(Some(c));
                    }
                    return LRESULT(1);
                }
            }
            WM_MOUSEMOVE => {
                with_app(|a| {
                    if !a.tracking_mouse {
                        let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: a.hwnd, dwHoverTime: 0 };
                        a.tracking_mouse = TrackMouseEvent(&mut tme).is_ok();
                    }
                    let (x, y) = a.to_dip(lparam);
                    if a.island.drag(x) | a.island.mouse_move(x, y) {
                        a.animate();
                    }
                });
                return LRESULT(0);
            }
            WM_MOUSELEAVE => {
                with_app(|a| {
                    a.tracking_mouse = false;
                    a.island.mouse_leave();
                    a.animate();
                });
                return LRESULT(0);
            }
            WM_APP_MEDIA => {
                if !HIDDEN.load(Ordering::Relaxed) {
                    with_app(|a| a.refresh());
                }
                return LRESULT(0);
            }
            // Over a fullscreen app too: the island shows, the launcher takes focus, and Windows raises the
            // taskbar itself, as on Win. Esc returns focus to the app, and the island hides again.
            WM_HOTKEY if wparam.0 as i32 == HOTKEY_LAUNCHER => {
                if vertical() {
                    return LRESULT(0);
                }
                with_app(|a| {
                    if HIDDEN.load(Ordering::Relaxed) {
                        a.unhide();
                    }
                    a.toggle_launcher();
                });
                return LRESULT(0);
            }
            // The key went to IME composition (Enter confirms CJK input rather than running a result).
            WM_KEYDOWN if wparam.0 == windows::Win32::UI::Input::KeyboardAndMouse::VK_PROCESSKEY.0 as usize => {}
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let handled = with_app(|a| {
                    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                    let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                    if a.island.menu_open() {
                        let e = a.island.menu_key(VIRTUAL_KEY(wparam.0 as u16), ctrl, shift);
                        a.menu_effect(e);
                        return true;
                    }
                    if !a.island.launcher_open() {
                        return false;
                    }
                    let e = a.island.launcher_key(VIRTUAL_KEY(wparam.0 as u16), ctrl, shift);
                    a.launcher_effect(e);
                    true
                });
                if handled == Some(true) {
                    return LRESULT(0);
                }
            }
            // A lone Alt puts the window into system menu mode, and letters arrive as WM_SYSCHAR, so the
            // launcher "stopped typing". The island has no menu, so the mode is not needed.
            WM_SYSCOMMAND if (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU => return LRESULT(0),
            // A character without Alt held is regular input, even if it came as a system one.
            WM_SYSCHAR if GetKeyState(VK_MENU.0 as i32) >= 0 && with_app(|a| a.island.launcher_open()) == Some(true) => {
                let _ = SendMessageW(hwnd, WM_CHAR, Some(wparam), Some(lparam));
                return LRESULT(0);
            }
            WM_CHAR if with_app(|a| a.island.menu_editing()) == Some(true) => {
                if let Some(c) = char::from_u32(wparam.0 as u32) {
                    with_app(|a| {
                        let e = a.island.menu_char(c);
                        a.menu_effect(e);
                    });
                }
                return LRESULT(0);
            }
            WM_CHAR => {
                // A character outside the BMP (emoji, rare CJK) arrives as two halves of a
                // surrogate pair; join them.
                let unit = wparam.0 as u16;
                let c = match HIGH_SURROGATE.with(|h| h.take()) {
                    _ if (0xD800..0xDC00).contains(&unit) => {
                        HIGH_SURROGATE.with(|h| h.set(Some(unit)));
                        None
                    }
                    Some(high) => char::decode_utf16([high, unit]).next().and_then(|r| r.ok()),
                    None => char::from_u32(unit as u32),
                };
                with_app(|a| {
                    if let (true, Some(c)) = (a.island.launcher_open(), c) {
                        let e = a.island.launcher_char(c);
                        a.launcher_effect(e);
                    }
                });
                return LRESULT(0);
            }
            // IME input (Chinese, Japanese): the composition and candidate windows sit at the launcher caret,
            // not in the window corner; the system draws the composition, and the final text
            // arrives as WM_CHAR.
            WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION => {
                with_app(|a| a.place_ime());
            }
            // Moved to another window (Alt+Tab, click): the launcher is no longer needed.
            WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
                with_app(|a| {
                    a.close_launcher(false);
                    a.close_menu(false);
                });
                a11y::focused(false);
            }
            WM_ACTIVATE => a11y::focused(true),
            WM_GETOBJECT => {
                let snapshot = with_app(|a| (a.island.access(), a.scale));
                if let Some(r) = a11y::get_object(wparam, lparam, snapshot) {
                    return r;
                }
            }
            WM_APP_A11Y_SYNC => {
                a11y::flush();
                return LRESULT(0);
            }
            WM_APP_A11Y_ACTION => {
                if let Some((x, y)) = a11y::center(wparam.0 as u64) {
                    with_app(|a| a.click_at(x, y));
                }
                return LRESULT(0);
            }
            WM_APP_TERM => {
                SetTimer(Some(hwnd), TIMER_TERM, TERM_SETTLE_MS, None);
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_TERM => {
                let _ = KillTimer(Some(hwnd), TIMER_TERM);
                let min = std::time::Duration::from_secs(TERMINAL_MIN.load(Ordering::Relaxed));
                for note in terminal::settle(min, foreground_pid()) {
                    with_app(|a| a.notify(note));
                }
                return LRESULT(0);
            }
            WM_APP_PUSH => {
                with_app(|a| a.apply_pushes());
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_WAKE => {
                let _ = KillTimer(Some(hwnd), TIMER_WAKE);
                with_app(|a| a.refresh());
                return LRESULT(0);
            }
            // DefWindowProc after handling is required: it frees the event buffer.
            WM_INPUT => {
                if let Some(note) = locks::on_input(lparam) {
                    with_app(|a| a.notify(note));
                }
            }
            WM_TIMER if wparam.0 == TIMER_NOTES => {
                with_app(|a| {
                    if a.island.expire_notes() {
                        a.sync_hook();
                        a.animate();
                    }
                    a.sync_note_timer();
                });
                return LRESULT(0);
            }
            WM_APP_FOREGROUND => {
                track_terminal();
                with_app(|a| a.sync_fullscreen());
                return LRESULT(0);
            }
            WM_MOUSEWHEEL => {
                let notches = ((wparam.0 >> 16) & 0xffff) as u16 as i16 as f32 / 120.0;
                with_app(|a| {
                    a.island.launcher_wheel(notches);
                    a.animate();
                });
                return LRESULT(0);
            }
            // Right click on the island is only the launcher row menu; the island's menu is in the tray: on
            // the island the right button was intercepted by widgets and the taskbar's system menu.
            WM_RBUTTONUP => {
                with_app(|a| {
                    let (x, y) = a.to_dip(lparam);
                    if let Some(e) = a.island.launcher_menu_at(x, y) {
                        a.launcher_effect(e);
                    }
                });
                return LRESULT(0);
            }
            WM_APP_TRAY => {
                let event = (lparam.0 & 0xffff) as u32;
                if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    show_menu(hwnd);
                }
                return LRESULT(0);
            }
            WM_APP_OUTSIDE => {
                with_app(|a| a.close());
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_CLOCK => {
                with_app(|a| {
                    let now = GetLocalTime();
                    // A visible track time ticks every second, the clock once a minute.
                    let ticking = a.island.needs_ticks();
                    if now.wMinute != a.minute || ticking {
                        a.minute = now.wMinute;
                        a.refresh();
                    }
                });
                // Fullscreen mode can start without a foreground change (F11 in a browser).
                with_app(|a| a.sync_fullscreen());
                // A safety net in case the event-driven raise was missed.
                raise();
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_INVOKE => {
                let _ = KillTimer(Some(hwnd), TIMER_INVOKE);
                with_app(|a| {
                    if a.invoking.take().is_some() {
                        a.close_launcher(true);
                    }
                });
                return LRESULT(0);
            }
            WM_APP_MODULES => {
                modules::reload(&modules::off(&config::load()), vertical());
                sync_hotkeys(hwnd);
                return LRESULT(0);
            }
            // A module command's hotkey: a `selection` command first copies the selection of the
            // foreground app, in the background, and comes back with `WM_APP_CAPTURED`.
            WM_HOTKEY if wparam.0 as i32 >= hotkeys::FIRST_ID => {
                match hotkeys::lookup(wparam.0 as i32) {
                    Some(b) if b.selection => hotkeys::grab(b, hwnd, WM_APP_CAPTURED),
                    Some(b) => {
                        with_app(|a| a.hotkey_invoke(b, String::new()));
                    }
                    None => {}
                }
                return LRESULT(0);
            }
            WM_APP_CAPTURED => {
                if let Some((b, selection)) = hotkeys::take() {
                    with_app(|a| a.hotkey_invoke(b, selection));
                }
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_PREWARM => {
                let _ = KillTimer(Some(hwnd), TIMER_PREWARM);
                with_app(|a| a.island.prewarm_launcher(a.gfx.dpi));
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_LOOK_BOOT => {
                let i = BOOT_LOOK.with(|b| b.get()) + 1;
                BOOT_LOOK.with(|b| b.set(i));
                let _ = KillTimer(Some(hwnd), TIMER_LOOK_BOOT);
                if let Some(next) = LOOK_BOOT_MS.get(i) {
                    SetTimer(Some(hwnd), TIMER_LOOK_BOOT, next - LOOK_BOOT_MS[i - 1], None);
                }
                with_app(|a| a.capture_look());
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_LOOK => {
                let _ = KillTimer(Some(hwnd), TIMER_LOOK);
                with_app(|a| a.capture_look());
                return LRESULT(0);
            }
            WM_APP_REFRESH => {
                with_app(|a| a.capture_look());
                return LRESULT(0);
            }
            WM_APP_PACKAGE => {
                modules::start_packaged(&modules::off(&config::load()));
                // The "Enable notifications" row in modules is no longer needed.
                with_app(|a| {
                    if a.island.menu_title(settings::MODULE) == Some(i18n::t().set_modules) {
                        a.island.menu_replace(settings::MODULE, settings::modules_screen());
                        a.animate();
                    }
                });
                return LRESULT(0);
            }
            // After lock and sleep the look may have changed without events, and after patches too.
            WM_WTSSESSION_CHANGE if wparam.0 == WTS_SESSION_UNLOCK => {
                request_look(hwnd);
            }
            WM_POWERBROADCAST if wparam.0 == PBT_APMRESUMEAUTOMATIC => {
                request_look(hwnd);
            }
            // Accent color.
            WM_DWMCOLORIZATIONCOLORCHANGED => {
                request_look(hwnd);
            }
            WM_SETTINGCHANGE if wparam.0 == SPI_SETHIGHCONTRAST.0 as usize => {
                request_look(hwnd);
            }
            WM_TIMER if wparam.0 == TIMER_REBUILD => {
                // The taskbar is still moving to the new edge (the move is animated): wait until it settles,
                // otherwise the window is built at an intermediate position.
                let now = tray_rect();
                if REBUILD_TRAY.with(|r| r.replace(now)) != now {
                    return LRESULT(0);
                }
                let _ = KillTimer(Some(hwnd), TIMER_REBUILD);
                proto::info!("core", "recreating window");
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            // Monitor scale or resolution changed: the window's size and DPI are stale. An external request
            // to exit: a new version replaces the exe, or taskkill without /f.
            WM_CLOSE => {
                QUITTING.store(true, Ordering::Relaxed);
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            WM_DPICHANGED | WM_DISPLAYCHANGE => {
                request_rebuild(hwnd);
                return LRESULT(0);
            }
            // The wallpaper changed: the glass is rebuilt. Windows 11 does not always broadcast it, so the
            // main source is watching the registry and the wallpaper folder.
            WM_SETTINGCHANGE if wparam.0 == SPI_SETDESKWALLPAPER.0 as usize => {
                with_app(|a| a.check_wallpaper());
                request_look(hwnd);
            }
            WM_APP_WALLPAPER => {
                with_app(|a| a.check_wallpaper());
                request_look(hwnd);
                return LRESULT(0);
            }
            // Theme, accent, transparency: a broadcast with the ImmersiveColorSet parameter.
            WM_SETTINGCHANGE if is_color_set(lparam) => {
                request_look(hwnd);
            }
            // The work area changes when the taskbar changes size.
            WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => {
                rebuild_if_taskbar_resized(hwnd);
                // The taskbar turned vertical or came back.
                with_app(|a| a.sync_fullscreen());
                modules::reconcile(&modules::off(&config::load()), vertical());
            }
            WM_DESTROY => {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_LAUNCHER);
                hotkeys::sync(hwnd, Vec::new());
                with_app(|a| {
                    if let Some(tid) = a.hook.take() {
                        stop_mouse_hook(tid);
                    }
                    let zone = a.island.take_zone();
                    ZONE_STASH.with(|z| *z.borrow_mut() = Some(zone));
                });
                remove_tray_icon(hwnd);
                HWND_ISLAND.store(0, Ordering::Relaxed);
                APP.with(|a| {
                    if let Ok(mut a) = a.try_borrow_mut() {
                        *a = None;
                    }
                });
                if QUITTING.load(Ordering::Relaxed) {
                    PostQuitMessage(0);
                } else {
                    // The window went away with the taskbar (Explorer restart): recreate it.
                    let _ = PostThreadMessageW(GetCurrentThreadId(), WM_APP_RESPAWN, WPARAM(0), LPARAM(0));
                }
                return LRESULT(0);
            }
            _ => {}
        }
        // Explorer restarted: the old taskbar vanished with the tray icon, and the window's owner was reset;
        // bind to the new taskbar.
        if msg != 0 && with_app(|a| a.taskbar_created) == Some(msg) {
            attach_to_taskbar(hwnd);
            add_tray_icon(hwnd);
            rebuild_if_taskbar_resized(hwnd);
            request_look(hwnd);
            // A new taskbar is drawn in steps, like at boot: a single capture can catch it half-painted.
            BOOT_LOOK.with(|b| b.set(0));
            SetTimer(Some(hwnd), TIMER_LOOK_BOOT, LOOK_BOOT_MS[0], None);
            // The window survived and still holds Alt+Space: registering it again would fail and
            // report a conflict that is not there.
            let _ = UnregisterHotKey(Some(hwnd), HOTKEY_LAUNCHER);
            // Alt+Space may be taken by another launcher; then the magnifier click remains.
            if RegisterHotKey(Some(hwnd), HOTKEY_LAUNCHER, MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32).is_err() {
                proto::warn!("core", "Alt+Space is taken by another app");
            }
            sync_hotkeys(hwnd);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Registers the hotkeys of enabled modules' commands, as the manifests and `config.ini` set them.
fn sync_hotkeys(hwnd: HWND) {
    let c = config::load();
    hotkeys::sync(hwnd, modules::hotkeys(&modules::off(&c), &c.hotkeys));
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
    }
}

/// Opens the native notification and calendar panel (Win+N), replacing a click on the system clock.
fn open_notification_center() {
    let seq = [key(VK_LWIN, false), key(VK_N, false), key(VK_N, true), key(VK_LWIN, true)];
    // SAFETY: `seq` is a valid array of fully initialized inputs that outlives the call.
    unsafe { SendInput(&seq, size_of::<INPUT>() as i32) };
}

/// The mark from the exe resources at small icon size for the window's DPI.
fn tray_icon(hwnd: HWND) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    // SAFETY: `hwnd` is our window; resource id 1 is the icon embedded by build.rs.
    unsafe {
        use windows::Win32::UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            WindowsAndMessaging::{HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXSMICON, SM_CYSMICON},
        };
        let dpi = GetDpiForWindow(hwnd).max(96);
        let (w, h) = (GetSystemMetricsForDpi(SM_CXSMICON, dpi), GetSystemMetricsForDpi(SM_CYSMICON, dpi));
        let instance = GetModuleHandleW(None).ok();
        LoadImageW(instance.map(Into::into), PCWSTR(1 as _), IMAGE_ICON, w, h, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: tray_icon(hwnd),
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip("Polynya".encode_utf16()) {
        *dst = src;
    }
    nid
}

fn add_tray_icon(hwnd: HWND) {
    // SAFETY: `NOTIFYICONDATAW` is fully initialized with its `cbSize` and outlives the call.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd));
    }
}

fn remove_tray_icon(hwnd: HWND) {
    // SAFETY: `NOTIFYICONDATAW` is fully initialized with its `cbSize` and outlives the call.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
    }
}

fn show_menu(hwnd: HWND) {
    // SAFETY: the menu is created and destroyed here; strings and `pt` outlive their calls.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let auto = if autostart::is_enabled() { MF_CHECKED } else { MF_UNCHECKED };
        let tr = i18n::t();
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, &HSTRING::from(tr.menu_settings));
        let _ = AppendMenuW(menu, MF_STRING | auto, ID_AUTOSTART, &HSTRING::from(tr.menu_autostart));
        let _ = AppendMenuW(menu, MF_STRING, ID_REFRESH, &HSTRING::from(tr.menu_refresh));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, &HSTRING::from(tr.menu_exit));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Without this the menu does not close on a click outside.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None).0 as usize;
        let _ = DestroyMenu(menu);

        match cmd {
            ID_SETTINGS => {
                // The island is hidden (vertical taskbar), nowhere to show the settings: open the config.
                if with_app(|a| a.open_settings()) != Some(true) {
                    let path = HSTRING::from(config::path().as_os_str());
                    ShellExecuteW(None, w!("open"), &path, None, None, SW_SHOWNORMAL);
                }
            }
            ID_AUTOSTART => autostart::set(!autostart::is_enabled()),
            ID_REFRESH => with_app(|a| a.capture_look()).unwrap_or(()),
            ID_EXIT => {
                QUITTING.store(true, Ordering::Relaxed);
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}
