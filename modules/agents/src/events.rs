// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Translates AI agent events into right-zone status chips.

use std::path::Path;

use proto::Msg;
use serde_json::Value;
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Lifetime of the final status chip (done/error) in seconds.
const DONE_TTL: u64 = 10;
/// Maximum length for chip details to maintain a compact UI.
const MAX_BODY: usize = 160;

/// Localized UI strings for agent statuses.
pub struct Strings {
    pub thinking: &'static str,
    pub waiting: &'static str,
    pub done: &'static str,
    pub edits: &'static str,
    pub reads: &'static str,
    pub searches: &'static str,
    pub error: &'static str,
}

const RU: Strings =
    Strings {
        thinking: "Думает…", waiting: "Ожидание…", done: "Готово", edits: "Редактирует", reads: "Читает", searches: "Поиск", error: "Ошибка"
    };
const EN: Strings =
    Strings { thinking: "Thinking…", waiting: "Waiting…", done: "Done", edits: "Editing", reads: "Reading", searches: "Searching", error: "Error" };
const ZH: Strings = Strings {
    thinking: "思考中…", waiting: "等待中…", done: "完成", edits: "编辑中", reads: "读取中", searches: "搜索中", error: "错误"
};
const JA: Strings =
    Strings {
        thinking: "思考中…", waiting: "応答待ち…", done: "完了", edits: "編集中", reads: "読み込み中", searches: "検索中", error: "エラー"
    };

pub fn strings(lang: &str) -> &'static Strings {
    match lang {
        "ru" => &RU,
        "zh" => &ZH,
        "ja" => &JA,
        _ => &EN,
    }
}

fn clock() -> String {
    // SAFETY: `GetLocalTime` has no preconditions.
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

/// Truncates to the first non-empty line, respecting MAX_BODY.
fn clip(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() > MAX_BODY { format!("{}…", line.chars().take(MAX_BODY).collect::<String>()) } else { line.to_string() }
}

/// Extracts the file name from a given path string.
fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Constructs an agent status chip message.
#[allow(clippy::too_many_arguments)]
fn chip(key: String, icon: &str, title: String, project: &str, detail: &str, source: &str, ttl: Option<u64>, animated: bool) -> Msg {
    let body = if detail.is_empty() { project.to_string() } else { format!("{project} · {}", clip(detail)) };
    Msg::Chip {
        key,
        source: None,
        icon: icon.into(),
        title,
        body,
        meta: format!("{source} · {}", clock()),
        ttl_ms: ttl.map(|t| t * 1000),
        animated,
        app: None,
        popup: false,
        swatches: Vec::new(),
        mark: None,
    }
}

/// Parses a Claude Code hook event.
///
/// One session is one chip: thinking (prompt, tool call) → waiting for the user (permission
/// request or question) → done. Session end removes the chip. The idle reminder after a reply
/// is skipped: "done" was already shown.
pub fn claude(v: &Value, tr: &Strings) -> Option<Msg> {
    let s = |k: &str| v[k].as_str().unwrap_or("");
    let key = format!("claude:{}", s("session_id"));
    let project = file_name(s("cwd"));
    let note = |title: &str, detail: String, ttl: Option<u64>, animated: bool| {
        chip(key.clone(), "claude", title.into(), &project, &detail, "Claude Code", ttl, animated)
    };
    Some(match s("hook_event_name") {
        "UserPromptSubmit" => note(tr.thinking, String::new(), None, true),
        "PreToolUse" if s("tool_name") == "AskUserQuestion" => {
            let q = &v["tool_input"]["questions"][0];
            note(tr.waiting, q["question"].as_str().or(q["header"].as_str()).unwrap_or("").to_string(), None, false)
        }
        "PreToolUse" | "PostToolUse" => note(tr.thinking, action(s("tool_name"), &v["tool_input"], tr), None, true),
        "Notification" => {
            if s("notification_type") == "idle_prompt" {
                return None;
            }
            note(tr.waiting, s("message").to_string(), None, false)
        }
        "Stop" => note(tr.done, String::new(), Some(DONE_TTL), false),
        "SessionEnd" => Msg::Dismiss { key },
        _ => return None,
    })
}

/// Parses a Cursor hook event.
///
/// Cursor has no permission request among its fire-and-forget events, so only thinking → done
/// or error. Cursor waits for a reply to the prompt submit event, so the module allows it to continue.
pub fn cursor(v: &Value, tr: &Strings) -> Option<Msg> {
    let s = |k: &str| v[k].as_str().unwrap_or("");
    let event = s("hook_event_name");
    if event == "beforeSubmitPrompt" {
        println!("{{\"continue\": true}}");
    }
    let key = format!("cursor:{}", s("conversation_id"));
    let project = file_name(v["workspace_roots"][0].as_str().unwrap_or(""));
    let note = |icon: &str, state: &str, detail: String, ttl: Option<u64>| {
        chip(key.clone(), icon, format!("Cursor · {state}"), &project, &detail, "Cursor", ttl, icon == "agent")
    };
    Some(match event {
        "beforeSubmitPrompt" => note("agent", tr.thinking, String::new(), None),
        "afterFileEdit" => note("agent", tr.thinking, format!("{} {}", tr.edits, file_name(s("file_path"))), None),
        "afterShellExecution" => note("agent", tr.thinking, s("command").to_string(), None),
        "stop" => match s("status") {
            "completed" => note("ok", tr.done, String::new(), Some(DONE_TTL)),
            "error" => note("fail", tr.error, String::new(), Some(DONE_TTL)),
            _ => Msg::Dismiss { key }, // Stopped by user, no notification needed
        },
        "sessionEnd" => Msg::Dismiss { key },
        _ => return None,
    })
}

/// Parses a Codex CLI event: its `notify` reports only the end of a turn. The event carries no
/// project folder, so the project is the working directory of the process that launched Codex.
pub fn codex(v: &Value, tr: &Strings) -> Option<Msg> {
    if v["type"].as_str() != Some("agent-turn-complete") {
        return None;
    }
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let project = file_name(&cwd);
    Some(chip(format!("codex:{project}"), "ok", format!("Codex · {}", tr.done), &project, "", "Codex", Some(DONE_TTL), false))
}

/// Returns a concise, human-readable description of the executed tool.
fn action(tool: &str, input: &Value, tr: &Strings) -> String {
    let field = |k: &str| input[k].as_str().unwrap_or("");
    let file = |k: &str| file_name(field(k));
    match tool {
        "Edit" | "MultiEdit" | "Write" => format!("{} {}", tr.edits, file("file_path")),
        "NotebookEdit" => format!("{} {}", tr.edits, file("notebook_path")),
        "Read" => format!("{} {}", tr.reads, file("file_path")),
        "Grep" | "Glob" => format!("{} {}", tr.searches, field("pattern")),
        "WebSearch" => format!("{} {}", tr.searches, field("query")),
        "Bash" | "PowerShell" => field("command").to_string(),
        "Task" | "Agent" => field("description").to_string(),
        _ => tool.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_actions() {
        let v = serde_json::json!({ "file_path": r"C:\Projects\PER-polynya\src\main.rs" });
        assert_eq!(action("Edit", &v, &EN), "Editing main.rs");
        assert_eq!(action("Bash", &serde_json::json!({ "command": "cargo build" }), &EN), "cargo build");
    }

    #[test]
    fn cursor_events() {
        let v = serde_json::json!({ "hook_event_name": "stop", "status": "completed", "conversation_id": "c1", "workspace_roots": [r"C:\Projects\site"] });
        let Some(Msg::Chip { key, title, body, .. }) = cursor(&v, &EN) else { panic!() };
        assert_eq!((key.as_str(), title.as_str(), body.as_str()), ("cursor:c1", "Cursor · Done", "site"));
        let v = serde_json::json!({ "hook_event_name": "stop", "status": "aborted", "conversation_id": "c1" });
        assert!(matches!(cursor(&v, &EN), Some(Msg::Dismiss { .. })));
    }

    #[test]
    fn clip_takes_first_line() {
        assert_eq!(clip("\n  first  \nsecond"), "first");
        assert!(clip(&"а".repeat(300)).ends_with('…'));
    }
}
