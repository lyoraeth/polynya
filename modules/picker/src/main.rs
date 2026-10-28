// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Polynya "Picker" module: captures screen color to clipboard and displays a chip with its shades.
//!
//! Started as `polynya-picker run` by the core, it waits for the `invoke` of its `pick` command. The screen is not frozen; the loupe shows live updates.
//! A click is caught by an invisible topmost window to prevent the click from reaching the app underneath.
//! Both windows are excluded from screen capture to prevent the picker from seeing itself.

#![windows_subsystem = "windows"]

mod shades;

use std::cell::RefCell;

use proto::Msg;
use windows::{
    Win32::{
        Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, AddFontMemResourceEx, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt, CAPTUREBLT,
            CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
            DeleteDC, DeleteObject, DrawTextW, FF_DONTCARE, FW_NORMAL, GetDC, GetMonitorInfoW, HBITMAP, HDC, HFONT, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS, ReleaseDC, SRCCOPY, SelectObject, SetBkColor, SetTextColor,
        },
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            LibraryLoader::GetModuleHandleW,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            SystemInformation::GetLocalTime,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::VK_ESCAPE,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, IDC_CROSS, KillTimer, LWA_ALPHA,
                LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
                SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity, ShowWindow,
                TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN,
                WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::w,
};

/// Number of pixels per side in the loupe (must be odd for a center pixel).
const N: i32 = 11;
/// Dimensions in DIPs: loupe diameter, border, HEX label, and gaps.
const LOUPE: f32 = 120.0;
const BORDER: f32 = 3.0;
const LABEL_W: f32 = 92.0;
const LABEL_H: f32 = 26.0;
const LABEL_GAP: f32 = 8.0;
const FONT_PX: f32 = 13.0;
/// Offset from the cursor to the loupe.
const OFFSET: f32 = 22.0;
/// Island UI colors: background plate and foreground text.
const PLATE: [u8; 3] = [0x18, 0x17, 0x1c];
const FG: [u8; 3] = [0xff, 0xff, 0xff];
/// Loupe update interval in ms (screen under cursor changes without mouse movement).
const TICK_MS: u32 = 16;
/// Lifetime of the result chip in ms.
const CHIP_TTL: u64 = 10_000;
/// Clipboard format for UTF-16 text.
const CF_UNICODETEXT: u32 = 13;

static INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");

/// State of the active picker session.
struct Picker {
    catch: HWND,
    loupe: HWND,
    font: Option<(HFONT, f32)>,
    color: [u8; 3],
}

thread_local! {
    static PICKER: RefCell<Option<Picker>> = const { RefCell::new(None) };
}

/// Waits on stdin for the core's `invoke` of `main`: the picker is its own interface. `false`:
/// stdin closed first.
fn wait_invoke() -> bool {
    use std::io::BufRead;
    std::io::stdin()
        .lock()
        .lines()
        .map_while(Result::ok)
        .any(|l| matches!(serde_json::from_str(&l), Ok(proto::ModIn::Invoke { command, .. }) if command == "main"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("run") || !wait_invoke() {
        return;
    }
    // The picker draws its own windows: the launcher closes right away.
    if let Ok(line) = serde_json::to_string(&proto::ModOut::Close) {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}").and_then(|()| out.flush());
    }
    // SAFETY: runs on the main thread, which owns the windows and the message loop; `INTER` is
    // static, and all other buffers outlive their calls.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mut fonts = 0u32;
        AddFontMemResourceEx(INTER.as_ptr().cast(), INTER.len() as u32, None, &raw mut fonts);
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let cursor = LoadCursorW(None, IDC_CROSS).unwrap_or_default();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("PolynyaPicker"),
            hCursor: cursor,
            ..Default::default()
        });
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (cw, ch) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        // Click catcher: nearly transparent, but not fully, to prevent click-through.
        let Ok(catch) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("PolynyaPicker"),
            w!("Polynya picker"),
            WS_POPUP,
            x,
            y,
            cw,
            ch,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let _ = SetLayeredWindowAttributes(catch, COLORREF(0), 1, LWA_ALPHA);
        let Ok(loupe) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            w!("PolynyaPicker"),
            w!("Polynya loupe"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        for h in [catch, loupe] {
            let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
        }
        PICKER.with(|p| *p.borrow_mut() = Some(Picker { catch, loupe, font: None, color: [0; 3] }));
        let _ = ShowWindow(catch, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(catch);
        let _ = ShowWindow(loupe, SW_SHOWNOACTIVATE);
        update();
        SetTimer(Some(catch), 1, TICK_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by the system on the UI thread with valid arguments, passed on unchanged.
    unsafe {
        match msg {
            WM_TIMER | WM_MOUSEMOVE => update(),
            WM_LBUTTONDOWN => finish(true),
            WM_RBUTTONDOWN => finish(false),
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => finish(false),
            WM_DESTROY => {
                if PICKER.with(|p| p.borrow().as_ref().is_some_and(|p| p.catch == hwnd)) {
                    PostQuitMessage(0);
                }
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
        LRESULT(0)
    }
}

/// Captures color under cursor and redraws the loupe.
fn update() {
    PICKER.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return };
        // SAFETY: runs on the UI thread that owns the picker windows; out pointers outlive their
        // calls, and the old font is deleted once before it is replaced.
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let (mut dpi, mut _dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);
            let s = dpi as f32 / 96.0;
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(monitor, &mut info);
            let Some(grid) = capture(pt) else { return };
            p.color = grid[(N / 2 * N + N / 2) as usize];
            // Recreate font only when DPI scale changes (e.g., cursor moved to another monitor).
            if p.font.is_none_or(|(_, fs)| fs != s) {
                if let Some((f, _)) = p.font.take() {
                    let _ = DeleteObject(f.into());
                }
                let font = CreateFontW(
                    -(FONT_PX * s).round() as i32,
                    0,
                    0,
                    0,
                    FW_NORMAL.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    ANTIALIASED_QUALITY,
                    FF_DONTCARE.0 as u32,
                    w!("Inter Medium"),
                );
                p.font = Some((font, s));
            }
            draw(p.loupe, pt, s, info.rcMonitor, &grid, p.color, p.font.unwrap().0);
        }
    });
}

/// Captures an N×N grid of screen pixels around the given point, returning RGB values.
fn capture(pt: POINT) -> Option<Vec<[u8; 3]>> {
    // SAFETY: `dib` returns `bits` pointing to the N×N pixels of `bmp`, read before `bmp` is
    // deleted; every GDI object is released exactly once.
    unsafe {
        let screen = GetDC(None);
        let (dc, bmp, bits) = dib(Some(screen), N, N)?;
        let old = SelectObject(dc, bmp.into());
        // CAPTUREBLT includes layered windows (the island itself, menus, tooltips).
        let ok = BitBlt(dc, 0, 0, N, N, Some(screen), pt.x - N / 2, pt.y - N / 2, SRCCOPY | CAPTUREBLT).is_ok();
        let px = std::slice::from_raw_parts(bits, (N * N) as usize).iter().map(|&v| [(v >> 16) as u8, (v >> 8) as u8, v as u8]).collect();
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        ok.then_some(px)
    }
}

/// Allocates a top-down 32-bit DIB in memory. Returns DC, bitmap handle, and pointer to BGRA pixels.
///
/// # Safety
/// `reference`, if given, must be a valid DC. The pixel pointer is valid only until the bitmap
/// is deleted; the caller must delete the bitmap and the DC.
unsafe fn dib(reference: Option<HDC>, w: i32, h: i32) -> Option<(HDC, HBITMAP, *mut u32)> {
    // SAFETY: `info` and `bits` outlive the calls; `reference` is valid per the contract.
    unsafe {
        let dc = CreateCompatibleDC(reference);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            }, // Negative height for top-down DIB
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        Some((dc, bmp, bits.cast()))
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Renders the loupe: a magnified circle of screen pixels with a centered target frame,
/// and a HEX label below it.
#[allow(clippy::too_many_arguments)]
unsafe fn draw(hwnd: HWND, pt: POINT, s: f32, mon: RECT, grid: &[[u8; 3]], color: [u8; 3], font: HFONT) {
    let d = (LOUPE * s).round() as i32;
    let (lw, lh, gap) = ((LABEL_W * s).round() as i32, (LABEL_H * s).round() as i32, (LABEL_GAP * s).round() as i32);
    let (w, h) = (d.max(lw), d + gap + lh);
    // SAFETY: `bits` points to the w×h pixels of `bmp` and is used only before `bmp` is deleted;
    // `hwnd` is our layered window, and every GDI object is released exactly once.
    unsafe {
        let Some((dc, bmp, bits)) = dib(None, w, h) else { return };
        let buf = std::slice::from_raw_parts_mut(bits, (w * h) as usize);
        // UpdateLayeredWindow requires pre-multiplied alpha.
        let put = |buf: &mut [u32], x: i32, y: i32, c: [u8; 3], a: f32| {
            let a8 = (a.clamp(0.0, 1.0) * 255.0).round() as u32;
            let pm = |v: u8| (v as u32 * a8 + 127) / 255;
            buf[(y * w + x) as usize] = (a8 << 24) | (pm(c[0]) << 16) | (pm(c[1]) << 8) | pm(c[2]);
        };

        // Draw loupe circle.
        let (ox, r, border) = ((w - d) / 2, d as f32 / 2.0, BORDER * s);
        let cell = d as f32 / N as f32;
        let c0 = (N / 2) as f32 * cell;
        for y in 0..d {
            for x in 0..d {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let dist = ((fx - r).powi(2) + (fy - r).powi(2)).sqrt();
                let cover = (r - dist + 0.5).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let px = if dist > r - border {
                    PLATE
                } else {
                    let (cx, cy) = ((fx / cell) as i32, (fy / cell) as i32);
                    let mut c = grid[(cy.min(N - 1) * N + cx.min(N - 1)) as usize];
                    // Pixel grid lines slightly darkened.
                    let (ex, ey) = (fx - cx as f32 * cell, fy - cy as f32 * cell);
                    if ex < 1.0 || ey < 1.0 {
                        c = mix(c, [0, 0, 0], 0.18);
                    }
                    // Center pixel border: light inside, dark outside for visibility on any background.
                    let inside = |pad: f32| fx >= c0 - pad && fx < c0 + cell + pad && fy >= c0 - pad && fy < c0 + cell + pad;
                    if inside(1.5 * s) && !inside(0.0) {
                        c = FG;
                    } else if inside(2.5 * s) && !inside(1.5 * s) {
                        c = mix(c, [0, 0, 0], 0.6);
                    }
                    c
                };
                put(buf, ox + x, y, px, cover);
            }
        }

        // HEX label: rounded pill shape, text rendered as grayscale coverage.
        let (lx, ly) = ((w - lw) / 2, d + gap);
        let rr = lh as f32 / 2.0;
        let hex = proto::hex(color);
        let text = render_text(&hex, font, lw, lh);
        for y in 0..lh {
            for x in 0..lw {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let qx = (fx - lw as f32 / 2.0).abs() - (lw as f32 / 2.0 - rr);
                let dist = (qx.max(0.0).powi(2) + (fy - rr).powi(2)).sqrt();
                let cover = (rr - dist + 0.5).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let t = text.as_ref().map_or(0.0, |t| t[(y * lw + x) as usize]);
                    put(buf, lx + x, ly + y, mix(PLATE, FG, t), cover);
                }
            }
        }

        // Position loupe bottom-right of cursor; flip if it exceeds monitor bounds.
        let off = (OFFSET * s).round() as i32;
        let mut pos = POINT { x: pt.x + off, y: pt.y + off };
        if pos.x + w > mon.right {
            pos.x = pt.x - off - w;
        }
        if pos.y + h > mon.bottom {
            pos.y = pt.y - off - h;
        }
        let old = SelectObject(dc, bmp.into());
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let _ =
            UpdateLayeredWindow(hwnd, None, Some(&pos), Some(&SIZE { cx: w, cy: h }), Some(dc), Some(&POINT::default()), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
    }
}

/// Renders text to a grayscale coverage map (0.0 to 1.0).
unsafe fn render_text(text: &str, font: HFONT, w: i32, h: i32) -> Option<Vec<f32>> {
    // SAFETY: `bits` points to the w×h pixels of `bmp`, read before `bmp` is deleted; `font` is a
    // live font, and the previous objects are selected back before cleanup.
    unsafe {
        let (dc, bmp, bits) = dib(None, w, h)?;
        let old = SelectObject(dc, bmp.into());
        let old_font: HGDIOBJ = SelectObject(dc, font.into());
        SetTextColor(dc, COLORREF(0x00ff_ffff));
        SetBkColor(dc, COLORREF(0));
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
        DrawTextW(dc, &mut wide, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        let cover = std::slice::from_raw_parts(bits, (w * h) as usize).iter().map(|&v| ((v >> 8) & 0xff) as f32 / 255.0).collect();
        SelectObject(dc, old_font);
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        Some(cover)
    }
}

/// Finalizes the picker. If `pick` is true, copies color to clipboard and sends a chip.
fn finish(pick: bool) {
    let Some(p) = PICKER.with(|p| p.borrow().as_ref().map(|p| (p.catch, p.loupe, p.color))) else { return };
    let (catch, loupe, color) = p;
    // SAFETY: both windows are ours on this thread; a stale handle only makes the calls fail.
    unsafe {
        let _ = KillTimer(Some(catch), 1);
        let _ = DestroyWindow(loupe);
    }
    if pick {
        let hex = proto::hex(color);
        copy(&hex);
        let (shades, mark) = shades::shades(color);
        let (copied, name) = match proto::language() {
            "ru" => ("скопирован", "Пипетка"),
            "zh" => ("已复制", "取色器"),
            "ja" => ("をコピーしました", "カラーピッカー"),
            _ => ("copied", "Color picker"),
        };
        // SAFETY: `GetLocalTime` has no preconditions.
        let t = unsafe { GetLocalTime() };
        let [r, g, b] = color;
        let _ = proto::send(&Msg::Chip {
            key: "picker".into(),
            source: None,
            icon: format!("color:{hex}"),
            title: format!("{hex} {copied}"),
            body: format!("rgb({r}, {g}, {b})"),
            meta: format!("{name} · {:02}:{:02}", t.wHour, t.wMinute),
            ttl_ms: Some(CHIP_TTL),
            animated: false,
            app: None,
            popup: false,
            swatches: shades.into_iter().map(proto::hex).collect(),
            mark: Some(mark),
        });
    }
    // SAFETY: `catch` is our window on this thread.
    unsafe {
        let _ = DestroyWindow(catch);
    }
}

/// Copies a UTF-16 string to the system clipboard.
fn copy(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard is opened and closed on this thread; `dst` points to `wide.len()` u16s
    // just allocated and locked, and ownership passes to the system only via `SetClipboardData`.
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let dst = GlobalLock(mem) as *mut u16;
            if !dst.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                let _ = GlobalUnlock(mem);
                let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}
