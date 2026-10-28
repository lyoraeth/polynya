// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Polynya "Agents" module: CLI entry point for AI agent status tracking.
//!
//! Usage:
//! - `polynya-agents register | unregister` (called by core to manage hooks)
//! - `polynya-agents claude | cursor` (called by agent hooks, reads JSON from stdin)
//! - `polynya-agents codex <event>` (called by Codex notify, event as last arg)
//!
//! The hook stays silent and always exits successfully: a module error must not get in the agent's way.

#![windows_subsystem = "windows"]

mod events;
mod hooks;

use std::io::Read;

use serde_json::Value;

fn stdin_json() -> Option<Value> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).ok()?;
    serde_json::from_str(&input).ok()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tr = events::strings(proto::language());
    let msg = match args.first().map(String::as_str) {
        Some("register") => return hooks::sync(true),
        Some("unregister") => return hooks::sync(false),
        Some("claude") => stdin_json().and_then(|v| events::claude(&v, tr)),
        Some("cursor") => stdin_json().and_then(|v| events::cursor(&v, tr)),
        Some("codex") => args.last().and_then(|a| serde_json::from_str::<Value>(a).ok()).and_then(|v| events::codex(&v, tr)),
        _ => None,
    };
    if let Some(msg) = msg {
        // Core not running: there is nobody to tell, and that is not an error.
        let _ = proto::send(&msg);
    }
}
