// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pipe server: messages from modules and `polynya push` (the protocol is the `proto` crate).
//!
//! The core listens on the pipe in a separate thread. A client writes one JSON message and
//! exits; the core parses it, queues it and wakes the UI thread.

use std::{sync::Mutex, time::Duration};

use windows::Win32::{
    Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE},
    Storage::FileSystem::{PIPE_ACCESS_INBOUND, ReadFile},
    System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
        PIPE_WAIT,
    },
};

use crate::{icons::Icon, island::zone::Note};

pub use proto::INSTANCE_MUTEX;
use proto::{MAX_MSG, Msg};

/// Parsed message for the UI thread.
pub enum Push {
    Notify(Note),
    Dismiss(String),
}

impl From<Msg> for Push {
    fn from(m: Msg) -> Self {
        match m {
            Msg::Chip { key, source, icon, title, body, meta, ttl_ms, animated, app, popup, swatches, mark, .. } => Push::Notify(Note {
                key,
                source,
                app,
                popup,
                icon: Icon::parse(&icon).unwrap_or(Icon::Info),
                title,
                body,
                meta,
                ttl: ttl_ms.map(Duration::from_millis),
                animated,
                swatches: swatches.iter().filter_map(|s| proto::parse_hex(s)).collect(),
                mark,
            }),
            Msg::Dismiss { key } => Push::Dismiss(key),
        }
    }
}

static QUEUE: Mutex<Vec<Push>> = Mutex::new(Vec::new());

/// Takes queued messages. The queue keeps them while the window is missing (being recreated).
pub fn drain() -> Vec<Push> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// Queues a message as if it came over the pipe: chips that module processes write to stdout,
/// and the core's own reports about modules.
pub fn inject(m: Msg) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(m.into());
    }
}

/// Starts listening; `notify` is called from the pipe thread after each message.
pub fn start(notify: fn()) {
    let _ = std::thread::Builder::new().name("ipc".into()).spawn(move || serve(notify));
}

fn serve(notify: fn()) {
    let name = proto::pipe();
    loop {
        // SAFETY: `name` is a valid null-terminated pipe name that outlives the call.
        let pipe = unsafe {
            CreateNamedPipeW(
                &name,
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                0,
                MAX_MSG as u32,
                0,
                None,
            )
        };
        if pipe.is_invalid() {
            // SAFETY: reads the calling thread's last-error value; no preconditions.
            proto::error!("ipc", "create pipe: {:?}", unsafe { GetLastError() });
            return;
        }
        let data = receive(pipe);
        // SAFETY: `pipe` is a valid handle created above and closed exactly once here.
        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
        match serde_json::from_slice::<Msg>(&data) {
            Ok(m) => {
                if let Ok(mut q) = QUEUE.lock() {
                    q.push(m.into());
                }
                notify();
            }
            Err(e) if !data.is_empty() => proto::warn!("ipc", "bad message: {e}"),
            Err(_) => {}
        }
    }
}

/// Waits for a client and reads everything it writes until it closes the pipe.
fn receive(pipe: HANDLE) -> Vec<u8> {
    let mut data = Vec::new();
    // SAFETY: `pipe` is a valid pipe handle owned by the caller; `buf` and `n` outlive each
    // synchronous read.
    unsafe {
        // A client may connect between creating the pipe and waiting; that is a success too.
        if ConnectNamedPipe(pipe, None).is_err() && GetLastError() != ERROR_PIPE_CONNECTED {
            return data;
        }
        let mut buf = [0u8; 4096];
        loop {
            let mut n = 0u32;
            if ReadFile(pipe, Some(&mut buf), Some(&mut n), None).is_err() || n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n as usize]);
            if data.len() > MAX_MSG {
                return Vec::new();
            }
        }
    }
    data
}
