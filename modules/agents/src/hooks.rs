// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Manages registration and deregistration of hooks in AI agent configuration files.

use std::{fs, path::PathBuf};

use serde_json::{Value, json};

const CLAUDE_EVENTS: [&str; 6] = ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop", "SessionEnd"];
const CURSOR_EVENTS: [&str; 5] = ["beforeSubmitPrompt", "afterFileEdit", "afterShellExecution", "stop", "sessionEnd"];

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// Returns the current executable's absolute path and its lowercase file name.
/// The file name identifies this module's hooks.
fn exe() -> Option<(String, String)> {
    let path = std::env::current_exe().ok()?;
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    Some((path.to_string_lossy().into_owned(), name))
}

/// Formats the executable path for cross-shell compatibility.
/// Forward slashes prevent bash escaping; quotes handle spaces for PowerShell.
fn command(exe: &str, agent: &str) -> String {
    let exe = exe.replace('\\', "/");
    if exe.contains(' ') { format!("\"{exe}\" {agent}") } else { format!("{exe} {agent}") }
}

/// Checks if a hook command belongs to this module.
fn is_ours(cmd: &str, exe_name: &str, agent: &str) -> bool {
    let c = cmd.to_lowercase();
    c.contains(exe_name) && c.ends_with(&format!(" {agent}"))
}

/// Synchronizes hooks for all detected agents (adds or removes them). Errors go to the log.
pub fn sync(add: bool) {
    let (Some(home), Some((exe, name))) = (home(), exe()) else { return };
    if home.join(".claude").is_dir() {
        update_json(&home.join(".claude").join("settings.json"), |v| merge_claude(v, &command(&exe, "claude"), &name, add));
    }
    if home.join(".cursor").is_dir() {
        update_json(&home.join(".cursor").join("hooks.json"), |v| merge_cursor(v, &command(&exe, "cursor"), &name, add));
    }
    let codex = home.join(".codex");
    if codex.is_dir() {
        let path = codex.join("config.toml");
        let text = fs::read_to_string(&path).unwrap_or_default();
        if let Some(new) = merge_codex(&text, &exe, &name, add) {
            write(&path, &new);
        }
    }
}

fn write(path: &PathBuf, text: &str) {
    match fs::write(path, text) {
        Ok(()) => proto::info!("agents", "hooks updated: {}", path.display()),
        Err(e) => proto::error!("agents", "hooks: {}: {e}", path.display()),
    }
}

/// Reads a JSON file (a missing file is an empty object), applies a mutation, and writes it
/// back only if changed. A file that fails to parse is left untouched: it is someone else's config.
fn update_json(path: &PathBuf, merge: impl FnOnce(&mut Value) -> bool) {
    let mut v = match fs::read_to_string(path) {
        Ok(t) => match serde_json::from_str::<Value>(&t) {
            Ok(v) if v.is_object() => v,
            _ => return proto::warn!("agents", "hooks: {} not parsed, skipped", path.display()),
        },
        Err(_) => json!({}),
    };
    if merge(&mut v)
        && let Ok(text) = serde_json::to_string_pretty(&v)
    {
        write(path, &(text + "\n"));
    }
}

/// Removes outdated hooks of this module from a list, keeping at most one matching `cmd`.
/// Claude nests hooks inside groups (`nested`). Returns whether a current hook was found
/// and whether the list changed.
fn prune(list: &mut Vec<Value>, cmd: &str, exe_name: &str, agent: &str, nested: bool) -> (bool, bool) {
    let mut found = false;
    let mut changed = false;
    let mut keep = |h: &Value| {
        let c = h["command"].as_str().unwrap_or("");
        if !is_ours(c, exe_name, agent) {
            return true;
        }
        if c == cmd && !found {
            found = true;
            return true;
        }
        changed = true;
        false
    };
    if nested {
        for group in list.iter_mut() {
            if let Some(hooks) = group["hooks"].as_array_mut() {
                hooks.retain(&mut keep);
            }
        }
        let before = list.len();
        list.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
        changed |= list.len() != before;
    } else {
        list.retain(keep);
    }
    (found, changed)
}

/// Merges agent hook events to the desired state, adding or removing module-specific hooks.
/// Events left empty after removal are deleted entirely.
#[allow(clippy::too_many_arguments)]
fn merge_events(
    hooks: &mut serde_json::Map<String, Value>,
    events: &[&str],
    cmd: &str,
    exe_name: &str,
    agent: &str,
    add: bool,
    nested: bool,
    hook: impl Fn(&str) -> Value,
) -> bool {
    let mut changed = false;
    for ev in events {
        if !add && !hooks.contains_key(*ev) {
            continue;
        }
        let list = hooks.entry(*ev).or_insert_with(|| json!([]));
        let Some(list) = list.as_array_mut() else { continue };
        let target = if add { cmd } else { "" };
        let (found, pruned) = prune(list, target, exe_name, agent, nested);
        changed |= pruned;
        if add && !found {
            list.push(hook(ev));
            changed = true;
        }
        if !add && pruned && list.is_empty() {
            hooks.remove(*ev);
        }
    }
    changed
}

fn merge_claude(v: &mut Value, cmd: &str, exe_name: &str, add: bool) -> bool {
    let obj = v.as_object_mut().unwrap();
    if !add && !obj.contains_key("hooks") {
        return false;
    }
    let Some(hooks) = obj.entry("hooks").or_insert_with(|| json!({})).as_object_mut() else { return false };
    merge_events(hooks, &CLAUDE_EVENTS, cmd, exe_name, "claude", add, true, |ev| {
        let hook = json!({ "type": "command", "command": cmd, "async": true, "timeout": 5 });
        // Matcher required for tool events to trigger universally
        if ev.ends_with("ToolUse") { json!({ "matcher": "*", "hooks": [hook] }) } else { json!({ "hooks": [hook] }) }
    })
}

fn merge_cursor(v: &mut Value, cmd: &str, exe_name: &str, add: bool) -> bool {
    let obj = v.as_object_mut().unwrap();
    if !add && !obj.contains_key("hooks") {
        return false;
    }
    let mut changed = false;
    if add && !obj.contains_key("version") {
        obj.insert("version".into(), json!(1));
        changed = true;
    }
    let Some(hooks) = obj.entry("hooks").or_insert_with(|| json!({})).as_object_mut() else { return changed };
    changed | merge_events(hooks, &CURSOR_EVENTS, cmd, exe_name, "cursor", add, false, |_| json!({ "command": cmd, "timeout": 5 }))
}

/// Updates Codex config.toml. Preserves foreign `notify` entries as Codex only supports one.
fn merge_codex(text: &str, exe: &str, exe_name: &str, add: bool) -> Option<String> {
    let line = format!("notify = ['{exe}', 'codex']");
    let mut lines: Vec<&str> = text.lines().collect();
    let top = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let ours = |l: &str| {
        let l = l.to_lowercase();
        l.contains(exe_name) && l.contains("codex")
    };
    match lines[..top].iter().position(|l| l.trim_start().starts_with("notify") && l.contains('=')) {
        Some(i) if add && lines[i] == line => None,
        Some(i) if ours(lines[i]) => {
            if add {
                lines[i] = &line;
            } else {
                lines.remove(i);
            }
            Some(lines.join("\n") + "\n")
        }
        Some(_) => {
            if add {
                proto::warn!("agents", "hooks: Codex config.toml already has notify, skipped");
            }
            None
        }
        None if add => {
            lines.insert(0, &line);
            Some(lines.join("\n") + "\n")
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "polynya-agents.exe";
    const CMD: &str = "C:/m/polynya-agents.exe claude";

    #[test]
    fn command_survives_bash() {
        assert_eq!(command(r"C:\m\polynya-agents.exe", "claude"), "C:/m/polynya-agents.exe claude");
        assert_eq!(command(r"C:\Program Files\polynya-agents.exe", "codex"), "\"C:/Program Files/polynya-agents.exe\" codex");
    }

    #[test]
    fn claude_merge_is_idempotent_and_keeps_foreign() {
        let mut v = json!({ "model": "opus", "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify.ps1" }] }] } });
        assert!(merge_claude(&mut v, CMD, EXE, true));
        assert!(!merge_claude(&mut v, CMD, EXE, true));
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "notify.ps1");
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(v.as_object().unwrap().keys().next().unwrap(), "model");
    }

    #[test]
    fn claude_unregister_keeps_foreign_and_drops_empty() {
        let mut v = json!({ "hooks": { "Stop": [{ "hooks": [{ "command": "notify.ps1" }] }] } });
        merge_claude(&mut v, CMD, EXE, true);
        assert!(merge_claude(&mut v, CMD, EXE, false));
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(v["hooks"].get("PreToolUse").is_none());
        assert!(!merge_claude(&mut v, CMD, EXE, false));
    }

    #[test]
    fn cursor_merge() {
        let cmd = "C:/m/polynya-agents.exe cursor";
        let mut v = json!({});
        assert!(merge_cursor(&mut v, cmd, EXE, true));
        assert_eq!(v["version"], 1);
        assert_eq!(v["hooks"]["stop"][0]["command"], cmd);
        assert!(!merge_cursor(&mut v, cmd, EXE, true));
        assert!(merge_cursor(&mut v, cmd, EXE, false));
        assert!(v["hooks"].as_object().unwrap().is_empty());
    }

    #[test]
    fn codex_notify() {
        let exe = r"C:\m\polynya-agents.exe";
        let new = merge_codex("model = \"o3\"\n\n[tui]\nx = 1\n", exe, EXE, true).unwrap();
        assert!(new.starts_with(r"notify = ['C:\m\polynya-agents.exe', 'codex']"));
        assert!(merge_codex(&new, exe, EXE, true).is_none());
        assert!(merge_codex("notify = ['my-script']\n", exe, EXE, true).is_none());
        let removed = merge_codex(&new, exe, EXE, false).unwrap();
        assert!(!removed.contains("notify"));
        assert!(merge_codex("notify = ['my-script']\n", exe, EXE, false).is_none());
    }
}
