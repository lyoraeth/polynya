// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Files and folders via Everything, if it is running. No own index is kept.
//!
//! The protocol is `EVERYTHING_IPC_QUERY2` from the Everything SDK: the query goes as a
//! `WM_COPYDATA` message to the Everything window, and the reply comes back the same way to the
//! window named in the query. All of it lives in a separate thread with its own message window, so
//! the UI thread waits for neither the query nor the reply. Queries are coalesced: after a pause in
//! typing only the last one is sent.

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW},
        UI::WindowsAndMessaging::{
            ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, HWND_MESSAGE, KillTimer, MSG,
            MSGFLT_ALLOW, PostMessageW, RegisterClassW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, SetTimer, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COPYDATA,
            WM_TIMER, WNDCLASSW,
        },
    },
    core::{PCWSTR, w},
};

/// Pause after the last key press before querying, ms.
const DEBOUNCE_MS: u32 = 70;
/// How many results to request.
const MAX_RESULTS: u32 = 6;

const COPYDATA_QUERY2W: usize = 18;
const REQUEST_NAME: u32 = 0x1;
const REQUEST_PATH: u32 = 0x2;
/// Sort by run count: items opened from the launcher rise higher.
const SORT_RUN_COUNT_DESCENDING: u32 = 20;
const COPYDATA_INC_RUN_COUNTW: usize = 24;
const ITEM_FOLDER: u32 = 0x1;
/// Reply tag: Everything returns it in `dwData`.
const REPLY_ID: u32 = 0x5049;

const WM_SEARCH: u32 = WM_APP + 1;
const TIMER_SEARCH: usize = 1;

#[derive(Debug, Clone)]
pub struct Hit {
    pub name: String,
    /// Containing folder.
    pub folder: String,
    pub is_folder: bool,
}

impl Hit {
    pub fn path(&self) -> String {
        format!(r"{}\{}", self.folder.trim_end_matches('\\'), self.name)
    }
}

struct State {
    /// Last requested text.
    wanted: String,
    hits: Vec<Hit>,
    wake: Option<fn()>,
}

static STATE: Mutex<State> = Mutex::new(State { wanted: String::new(), hits: Vec::new(), wake: None });
static GENERATION: AtomicU64 = AtomicU64::new(0);
static WINDOW: OnceLock<AtomicUsize> = OnceLock::new();

/// Whether Everything is running; without it results have no files.
pub fn available() -> bool {
    everything().is_some()
}

fn everything() -> Option<HWND> {
    // SAFETY: the class name is a static null-terminated string.
    unsafe { FindWindowW(w!("EVERYTHING_TASKBAR_NOTIFICATION"), PCWSTR::null()).ok() }
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The last received reply. While the reply to a new query is in flight, results show the
/// previous one, so rows do not blink on every key press.
pub fn latest() -> Vec<Hit> {
    STATE.lock().map(|s| s.hits.clone()).unwrap_or_default()
}

/// Requests files for `query`; `wake` wakes the UI thread when the reply arrives.
pub fn search(query: &str, wake: fn()) {
    if let Ok(mut s) = STATE.lock() {
        if s.wanted == query {
            return;
        }
        s.wanted = query.to_string();
        s.wake = Some(wake);
    }
    let hwnd = WINDOW.get_or_init(spawn).load(Ordering::Relaxed);
    if hwnd != 0 {
        // SAFETY: posting to a window handle has no memory-safety preconditions; a stale handle just fails.
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as _)), WM_SEARCH, WPARAM(0), LPARAM(0));
        }
    }
}

/// Tells Everything the file was opened, so it ranks higher in later results.
pub fn opened(path: &str) {
    let Some(ev) = everything() else { return };
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let cds = COPYDATASTRUCT { dwData: COPYDATA_INC_RUN_COUNTW, cbData: (wide.len() * 2) as u32, lpData: wide.as_ptr() as _ };
    // SAFETY: `cds` and the `wide` buffer it points to outlive the synchronous send.
    unsafe {
        let _ = SendMessageTimeoutW(ev, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG, 200, None);
    }
}

fn spawn() -> AtomicUsize {
    let (tx, rx) = std::sync::mpsc::channel();
    // SAFETY: the window class and window are created and used only on this thread; `msg` outlives
    // the message loop.
    let _ = std::thread::Builder::new().name("everything".into()).spawn(move || unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("Polynya.everything");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: class, ..Default::default() });
        // Message-only window: invisible and not part of window enumeration.
        let hwnd =
            CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR::null(), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(instance.into()), None);
        // An island run as admin would not get the reply otherwise: Windows blocks messages from a
        // normal-rights process (Everything) to an elevated one.
        if let Ok(h) = hwnd {
            let _ = ChangeWindowMessageFilterEx(h, WM_COPYDATA, MSGFLT_ALLOW, None);
        }
        let _ = tx.send(hwnd.map(|h| h.0 as usize).unwrap_or(0));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
    AtomicUsize::new(rx.recv().unwrap_or(0))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by the system with valid arguments; for `WM_COPYDATA`, `lparam` points to a
    // `COPYDATASTRUCT` whose `lpData` holds `cbData` bytes for the duration of the message.
    unsafe {
        match msg {
            WM_SEARCH => {
                SetTimer(Some(hwnd), TIMER_SEARCH, DEBOUNCE_MS, None);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_SEARCH => {
                let _ = KillTimer(Some(hwnd), TIMER_SEARCH);
                query(hwnd);
                LRESULT(0)
            }
            WM_COPYDATA => {
                let cds = &*(lparam.0 as *const COPYDATASTRUCT);
                if cds.dwData == REPLY_ID as usize {
                    let bytes = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
                    answer(parse(bytes));
                }
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn query(reply: HWND) {
    let Some(text) = STATE.lock().ok().map(|s| s.wanted.clone()) else { return };
    let Some(ev) = everything().filter(|_| !text.trim().is_empty()) else {
        return answer(Vec::new());
    };
    // EVERYTHING_IPC_QUERY2: seven DWORDs followed by the null-terminated search string.
    let head = [reply.0 as usize as u32, REPLY_ID, 0, 0, MAX_RESULTS, REQUEST_NAME | REQUEST_PATH, SORT_RUN_COUNT_DESCENDING];
    let mut buf: Vec<u8> = head.iter().flat_map(|d| d.to_le_bytes()).collect();
    buf.extend(text.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()));
    let cds = COPYDATASTRUCT { dwData: COPYDATA_QUERY2W, cbData: buf.len() as u32, lpData: buf.as_ptr() as _ };
    // The reply arrives as a separate WM_COPYDATA to our window, while we wait here or later.
    // SAFETY: `cds` and `buf` outlive the synchronous send; Everything copies the data before returning.
    unsafe {
        let _ = SendMessageTimeoutW(ev, WM_COPYDATA, WPARAM(reply.0 as usize), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG, 1000, None);
    }
}

fn answer(hits: Vec<Hit>) {
    let wake = STATE.lock().ok().and_then(|mut s| {
        s.hits = hits;
        s.wake
    });
    GENERATION.fetch_add(1, Ordering::Relaxed);
    if let Some(w) = wake {
        w();
    }
}

fn dword(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A string in item data: a DWORD length in characters followed by the null-terminated text.
fn text_at(b: &[u8], at: usize) -> Option<(String, usize)> {
    let len = dword(b, at)? as usize;
    let start = at + 4;
    let raw = b.get(start..start + len * 2)?;
    let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some((String::from_utf16_lossy(&units), start + (len + 1) * 2))
}

/// Parses EVERYTHING_IPC_LIST2: a five-DWORD header, then items (flags, data offset from the list
/// start), then the data: name and folder in the order of the query flags.
fn parse(b: &[u8]) -> Vec<Hit> {
    let Some(count) = dword(b, 4) else { return Vec::new() };
    (0..count as usize)
        .filter_map(|i| {
            let item = 20 + i * 8;
            let flags = dword(b, item)?;
            let data = dword(b, item + 4)? as usize;
            let (name, next) = text_at(b, data)?;
            let (folder, _) = text_at(b, next)?;
            Some(Hit { name, folder, is_folder: flags & ITEM_FOLDER != 0 })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live query to a running Everything: `cargo test -- --ignored everything_live`.
    #[test]
    #[ignore]
    fn everything_live() {
        fn wake() {}
        search("polynya", wake);
        for _ in 0..50 {
            if !latest().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        println!("window {:?}, generation {}, hits {}", WINDOW.get().map(|w| w.load(Ordering::Relaxed)), generation(), latest().len());
        for h in latest().iter().take(3) {
            println!("{} | {}", h.name, h.folder);
        }
        assert!(!latest().is_empty());
    }

    fn text(s: &str) -> Vec<u8> {
        let mut v = (s.encode_utf16().count() as u32).to_le_bytes().to_vec();
        v.extend(s.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()));
        v
    }

    #[test]
    fn parses_list2() {
        // Header: totitems, numitems, offset, request_flags, sort_type.
        let mut b: Vec<u8> = [5u32, 1, 0, 3, 20].iter().flat_map(|d| d.to_le_bytes()).collect();
        let data_at = 20 + 8;
        b.extend(ITEM_FOLDER.to_le_bytes());
        b.extend((data_at as u32).to_le_bytes());
        b.extend(text("Проекты"));
        b.extend(text(r"C:\Users\Danil"));
        let hits = parse(&b);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path(), r"C:\Users\Danil\Проекты");
        assert!(hits[0].is_folder);
    }
}
