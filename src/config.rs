// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! User config: `config.ini` in [`proto::data_dir`] (`%APPDATA%\Polynya`), lines `key = value`.

use std::{fs, path::PathBuf};

use crate::i18n::Lang;

#[derive(Debug, Clone)]
pub struct Config {
    /// Manual taskbar height in DIPs; `None` detects it automatically.
    pub taskbar_height: Option<f32>,
    /// City for the weather; `None` uses the Windows location.
    pub city: Option<String>,
    /// Interface language; `None` uses the Windows language.
    pub language: Option<Lang>,
    /// Notify about terminal commands longer than this many seconds; 0 disables it.
    pub terminal_notify: u64,
    /// Disabled modules: ids from their manifests.
    pub modules_off: Vec<String>,
    /// Enabled experimental modules: without this they are off.
    pub modules_on: Vec<String>,
    /// Windows notifications pop up in the island instead of system banners (`notify_banners
    /// = ours`); `system` keeps the system banners, and the island shows only chips.
    pub notify_ours: bool,
    /// Interface font; `None` or not installed falls back to the embedded Inter.
    pub font: Option<String>,
    pub theme: Theme,
    /// Hotkeys of module commands set by the user: `hotkey.<module>.<command> = Ctrl+Alt+D`, as
    /// `("module.command", keys)`. Empty keys turn the manifest's hotkey off.
    pub hotkeys: Vec<(String, String)>,
}

/// Theme: follows the Windows taskbar or is forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Os,
    Dark,
    Light,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            taskbar_height: None,
            city: None,
            language: None,
            terminal_notify: 10,
            modules_off: Vec::new(),
            modules_on: Vec::new(),
            notify_ours: true,
            font: None,
            theme: Theme::Os,
            hotkeys: Vec::new(),
        }
    }
}

const DEFAULT_FILE: &str = "\
# Polynya
# Taskbar height in DIPs (pixels at 100% scale). Empty: taken from the system.
taskbar_height =
# City for the weather. Empty: the Windows location (if apps are allowed to use it).
city =
# Interface language: ru | en | zh | ja. Empty: the Windows language.
language =
# Notify about terminal commands longer than this many seconds; 0 disables it.
terminal_notify = 10
# Disabled modules, space-separated, e.g.: agents
modules_off =
# Enabled experimental modules, space-separated, e.g.: notify
modules_on =
# Windows notifications (notify module): ours pops them up in the island instead of Windows
# banners; system keeps the system banners, and the island shows only chips.
notify_banners = ours
# Text and icon color: os follows the Windows theme; dark | light. The island's background
# and plates adapt to the taskbar and the accent color by themselves.
theme = os
# Interface font: the name of a family installed in the system. Empty: the embedded Inter.
font =
# Module hotkeys override the module's own: hotkey.<module>.<command> = Ctrl+Alt+D;
# empty turns the hotkey off. E.g.: hotkey.devtools.convert = Ctrl+Shift+Q
# Log: %LOCALAPPDATA%\\Polynya\\logs\\polynya.log, in a portable copy logs\\ next to polynya.exe.
# debug is verbose, with track titles and notification app names; empty is the regular log.
log =
";

pub fn path() -> PathBuf {
    proto::data_dir().join("config.ini")
}

/// Writes `key = value` to the config: replaces the line with this key, otherwise appends it.
/// Comments and other lines stay as they were.
pub fn set(key: &str, value: &str) {
    let path = path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    let line = if value.is_empty() { format!("{key} =") } else { format!("{key} = {value}") };
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let is_key = !l.trim_start().starts_with('#') && l.split_once('=').is_some_and(|(k, _)| k.trim() == key);
            if is_key && !found {
                found = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(line);
    }
    let _ = fs::write(&path, out.join("\n") + "\n");
}

/// Reads the config; if the file is missing, creates it with default values.
pub fn load() -> Config {
    let p = path();
    let text = match fs::read_to_string(&p) {
        Ok(t) => t,
        Err(_) => {
            if let Some(dir) = p.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&p, DEFAULT_FILE);
            DEFAULT_FILE.to_string()
        }
    };
    parse(&text)
}

fn parse(text: &str) -> Config {
    let mut c = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "taskbar_height" => c.taskbar_height = v.parse().ok().filter(|h: &f32| *h > 0.0),
            "city" => c.city = Some(v.to_string()).filter(|c| !c.is_empty()),
            "language" => c.language = Lang::parse(v),
            "terminal_notify" => c.terminal_notify = v.parse().unwrap_or(c.terminal_notify),
            "modules_off" => c.modules_off = v.split([' ', ',']).filter(|m| !m.is_empty()).map(String::from).collect(),
            "modules_on" => c.modules_on = v.split([' ', ',']).filter(|m| !m.is_empty()).map(String::from).collect(),
            "notify_banners" => c.notify_ours = v != "system",
            "theme" => {
                c.theme = match v {
                    "dark" => Theme::Dark,
                    "light" => Theme::Light,
                    _ => Theme::Os,
                }
            }
            "font" => c.font = Some(v.to_string()).filter(|f| !f.is_empty()),
            _ if k.starts_with("hotkey.") => c.hotkeys.push((k["hotkey.".len()..].to_string(), v.to_string())),
            _ => {}
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_file() {
        let c = parse(DEFAULT_FILE);
        assert_eq!(c.taskbar_height, None);
        assert_eq!(c.city, None);
        assert_eq!(c.theme, Theme::Os);
    }

    #[test]
    fn theme() {
        assert_eq!(parse("theme = light\n").theme, Theme::Light);
        assert_eq!(parse("theme = dark\n").theme, Theme::Dark);
        assert_eq!(parse("theme = nonsense\n").theme, Theme::Os);
    }

    #[test]
    fn parses_overrides() {
        let c = parse("taskbar_height = 40\nterminal_notify = 0\n");
        assert_eq!(c.taskbar_height, Some(40.0));
        assert_eq!(c.terminal_notify, 0);
        let c = parse("hotkey.devtools.convert = Ctrl+Shift+Q\nhotkey.picker.pick =\n");
        assert_eq!(c.hotkeys, [("devtools.convert".into(), "Ctrl+Shift+Q".into()), ("picker.pick".into(), String::new())]);
    }
}
