// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings: a core dropdown on the same engine as module dropdowns. Screens are described as data
//! (`proto::Screen`), and selections come here instead of a module process.
//!
//! Basics live here; advanced options (font, taskbar height) are only in `config.ini`, with a button
//! to it at the bottom. Whatever applies on the fly is applied by the caller via [`Change`]; language
//! and modules need a restart, and then "Restart" appears as the first item.

use std::sync::atomic::{AtomicBool, Ordering};

use proto::{Row, Screen};

use crate::{
    config::{self, Theme},
    i18n::{self, Lang},
    modules,
};

/// The settings dropdown's "module".
pub const MODULE: &str = "settings";
const SOURCE: &str = "https://github.com/lyoraeth/polynya";
/// Long commands: notification threshold options, s; 0 is off.
const TERMINAL: [u64; 4] = [0, 10, 30, 60];

/// Something changed that takes effect only after a restart.
static RESTART: AtomicBool = AtomicBool::new(false);
/// Config keys that take effect after a restart.
const ON_RESTART: [&str; 4] = ["language", "modules_off", "modules_on", "notify_banners"];
/// Their values before the first change: "Discard changes" restores them.
static ORIGINAL: std::sync::Mutex<Option<Vec<(&'static str, String)>>> = std::sync::Mutex::new(None);
/// The city is being typed but not entered yet: the "by location" toggle is already off, while the
/// config stays untouched until Enter.
static MANUAL_CITY: AtomicBool = AtomicBool::new(false);

/// What to apply right away, without waiting for a restart.
pub enum Change {
    Theme(Theme),
    City(Option<String>),
    Terminal(u64),
    /// A module was switched on or off.
    Module(String),
}

/// What to do with the dropdown.
pub enum Act {
    /// New values for the current screen; `root` also for the first one (its labels depend on
    /// nested screens).
    Replace {
        screen: Screen,
        root: Option<Screen>,
    },
    Push(Screen),
    Restart,
    Close,
    None,
}

fn item(id: &str, text: &str, hint: String) -> Row {
    Row::Item { id: id.into(), text: text.into(), hint, keys: String::new(), command: String::new(), confirm: None }
}

/// The "Settings" row in search; opens them right there as a search page.
pub fn command() -> modules::Command {
    let tr = i18n::t();
    modules::Command {
        id: MODULE.into(),
        title: tr.menu_settings.into(),
        hint: "Polynya".into(),
        // All languages at once: search by the familiar word, not by the interface language.
        keywords: ["настройки", "параметры", "settings", "preferences", "config", "polynya", "设置", "設定"].map(String::from).to_vec(),
        // Lucide «settings-2»
        icon: "M20 7h-9|M14 17H5|M20 17a3 3 0 1 1-6 0a3 3 0 1 1 6 0|M10 7a3 3 0 1 1-6 0a3 3 0 1 1 6 0".into(),
        prefix: None,
        hotkey: None,
        selection: false,
        search: true,
    }
}

/// Whether changes are waiting for "Apply".
pub fn pending() -> bool {
    RESTART.load(Ordering::Relaxed)
}

/// Warning screen when closing with unapplied changes. "Back" returns to the settings; closing
/// it discards them.
pub fn guard() -> Screen {
    let tr = i18n::t();
    Screen {
        title: Some(tr.unsaved_title.into()),
        rows: vec![
            Row::Text { text: tr.unsaved_text.into() },
            Row::Button { id: "restart".into(), text: tr.apply.into(), plain: false },
            Row::Button { id: "discard".into(), text: tr.discard.into(), plain: true },
        ],
    }
}

/// Title of the warning screen; tells that it is already open.
pub fn is_guard(title: Option<&str>) -> bool {
    title == Some(i18n::t().unsaved_title)
}

/// Discards changes waiting for a restart: language and modules stay as they were.
pub fn discard() {
    let c = config::load();
    let packaged: Vec<String> = modules::scan().into_iter().filter(|m| m.package).map(|m| m.id).collect();
    for (key, value) in ORIGINAL.lock().ok().and_then(|mut o| o.take()).unwrap_or_default() {
        // The remembered module lists hold only packaged modules; the rest switched on the fly and
        // stay as they are now, or a discard would turn disabled modules back on.
        let value = match key {
            "modules_off" | "modules_on" => {
                let now = if key == "modules_off" { &c.modules_off } else { &c.modules_on };
                let rest = now.iter().filter(|id| !packaged.contains(id)).map(String::as_str);
                rest.chain(value.split(' ').filter(|id| !id.is_empty())).collect::<Vec<_>>().join(" ")
            }
            _ => value,
        };
        config::set(key, &value);
    }
    RESTART.store(false, Ordering::Relaxed);
}

/// Values of the [`ON_RESTART`] keys in the config.
fn on_restart(c: &config::Config) -> Vec<(&'static str, String)> {
    // Only packaged modules wait for a restart; the rest switch on the fly.
    let packaged: Vec<String> = modules::scan().into_iter().filter(|m| m.package).map(|m| m.id).collect();
    let only = |list: &[String]| list.iter().filter(|id| packaged.contains(id)).cloned().collect::<Vec<_>>().join(" ");
    let values = [lang_code(c.language).to_string(), only(&c.modules_off), only(&c.modules_on), banners(c.notify_ours).into()];
    ON_RESTART.into_iter().zip(values).collect()
}

/// Remembers the values before the first change that needs a restart.
fn remember(c: &config::Config) {
    if let Ok(mut o) = ORIGINAL.lock()
        && o.is_none()
    {
        *o = Some(on_restart(c));
    }
}

/// After a change, a restart is needed only if something differs from before: switched on and
/// right back off means nothing to apply.
fn settle() {
    let Ok(mut o) = ORIGINAL.lock() else { return };
    let same = o.as_ref().is_some_and(|orig| *orig == on_restart(&config::load()));
    if same {
        *o = None;
    }
    RESTART.store(!same, Ordering::Relaxed);
}

fn banners(ours: bool) -> &'static str {
    if ours { "ours" } else { "system" }
}

fn lang_code(l: Option<Lang>) -> &'static str {
    match l {
        None => "",
        Some(Lang::Ru) => "ru",
        Some(Lang::En) => "en",
        Some(Lang::Zh) => "zh",
        Some(Lang::Ja) => "ja",
    }
}

/// The notifications module package is installed and points to its folder.
fn notify_ready() -> bool {
    modules::dir().is_some_and(|d| matches!(crate::package::state(&d.join("notify")), crate::package::State::Ready(_)))
}

/// The first screen.
pub fn main() -> Screen {
    let tr = i18n::t();
    let c = config::load();
    let mut rows = Vec::new();
    let section = |t: &str| Row::Section { title: t.into() };
    let theme = match c.theme {
        Theme::Os => tr.as_windows,
        Theme::Dark => tr.theme_dark,
        Theme::Light => tr.theme_light,
    };
    rows.push(section(tr.sec_look));
    rows.push(item("theme", tr.set_theme, theme.into()));
    rows.push(item("lang", tr.set_language, lang_name(c.language).into()));
    rows.push(section(tr.sec_weather));
    let city = c.city.clone().unwrap_or_else(|| tr.city_auto.into());
    rows.push(item("city", tr.set_city, city));
    rows.push(section(tr.sec_behaviour));
    let all = modules::scan();
    let off = modules::off(&c);
    let on = all.iter().filter(|m| !off.contains(&m.id)).count();
    rows.push(item("modules", tr.set_modules, format!("{on} {} {}", tr.of, all.len())));
    rows.push(item("terminal", tr.set_terminal, terminal_name(c.terminal_notify)));
    rows.push(Row::Toggle { id: "autostart".into(), text: tr.menu_autostart.into(), on: crate::autostart::is_enabled() });
    // Applying language and modules means a restart: right after the settings, before the utility rows.
    if RESTART.load(Ordering::Relaxed) {
        rows.push(Row::Button { id: "restart".into(), text: tr.apply.into(), plain: false });
    }
    rows.push(Row::Divider);
    rows.push(item("config", tr.open_config, String::new()));
    rows.push(item("about", tr.about, String::new()));
    Screen { title: Some(tr.menu_settings.into()), rows }
}

/// Modules: a toggle each. An experimental one is a group: a header with its name, "Activate",
/// its settings under it when on, and a gap after.
pub fn modules_screen() -> Screen {
    let tr = i18n::t();
    let c = config::load();
    let off = modules::off(&c);
    let mut rows = Vec::new();
    for m in modules::scan() {
        let on = !off.contains(&m.id);
        let id = format!("mod.{}", m.id);
        // a script module without its interpreter: say what to install
        let name = match m.missing() {
            Some(what) => format!("{} · {}", m.name, tr.module_needs.replace("{}", what)),
            None => m.name.clone(),
        };
        if !m.experimental {
            rows.push(Row::Toggle { on, id, text: name });
            continue;
        }
        rows.push(Row::Section { title: m.name });
        rows.push(Row::Toggle { on, id, text: tr.activate_experimental.into() });
        // Windows notifications: without the package, install it first.
        if m.package && on {
            if !notify_ready() {
                rows.push(Row::Item {
                    id: "notify.setup".into(),
                    text: tr.notify_setup.into(),
                    hint: tr.notify_admin.into(),
                    keys: String::new(),
                    command: String::new(),
                    confirm: None,
                });
            }
            rows.push(Row::Toggle { id: "notify.banners".into(), text: tr.notify_banners.into(), on: c.notify_ours });
        }
        rows.push(Row::Section { title: String::new() });
    }
    rows.push(Row::Divider);
    rows.push(item("mod.folder", tr.modules_folder, String::new()));
    Screen { title: Some(tr.set_modules.into()), rows }
}

fn city_screen() -> Screen {
    let tr = i18n::t();
    let city = config::load().city;
    let auto = city.is_none() && !MANUAL_CITY.load(Ordering::Relaxed);
    let mut rows = vec![Row::Toggle { id: "city.auto".into(), text: tr.city_locate.into(), on: auto }];
    if !auto {
        rows.push(Row::Input { id: "city.name".into(), text: city.unwrap_or_default(), placeholder: tr.city_placeholder.into() });
    }
    Screen { title: Some(tr.set_city.into()), rows }
}

fn about_screen() -> Screen {
    let tr = i18n::t();
    Screen {
        title: Some(tr.about.into()),
        rows: vec![
            Row::Header { title: "Polynya".into(), subtitle: format!("{} {}", tr.version, env!("POLYNYA_VERSION")) },
            Row::Text {
                text: "© 2026 Danil Klimov (lyoraeth) & contributors
GPL-3.0"
                    .into(),
            },
            Row::Button { id: "about.source".into(), text: format!("{} · GitHub", tr.source), plain: false },
        ],
    }
}

fn lang_name(l: Option<Lang>) -> &'static str {
    match l {
        None => i18n::t().as_windows,
        Some(Lang::Ru) => "Русский",
        Some(Lang::En) => "English",
        Some(Lang::Zh) => "中文",
        Some(Lang::Ja) => "日本語",
    }
}

fn terminal_name(secs: u64) -> String {
    let tr = i18n::t();
    match secs {
        0 => tr.off.into(),
        s if s % 60 == 0 => format!("{} {}", s / 60, tr.unit_min),
        s => format!("{s} {}", tr.unit_s),
    }
}

/// Next value in a cycle.
fn next<T: PartialEq + Copy>(all: &[T], now: T) -> T {
    all.iter().position(|v| *v == now).map_or(all[0], |i| all[(i + 1) % all.len()])
}

/// Selection of item `id` (or `text` entered in a field): edits the config and tells what to show
/// and what to apply right away.
pub fn select(id: &str, text: Option<String>) -> (Act, Option<Change>) {
    let c = config::load();
    let here = |s: Screen| Act::Replace { screen: s, root: None };
    match id {
        "theme" => {
            let t = next(&[Theme::Os, Theme::Dark, Theme::Light], c.theme);
            config::set(
                "theme",
                match t {
                    Theme::Os => "os",
                    Theme::Dark => "dark",
                    Theme::Light => "light",
                },
            );
            (here(main()), Some(Change::Theme(t)))
        }
        "lang" => {
            remember(&c);
            let l = next(&[None, Some(Lang::Ru), Some(Lang::En), Some(Lang::Zh), Some(Lang::Ja)], c.language);
            config::set("language", lang_code(l));
            settle();
            (here(main()), None)
        }
        "terminal" => {
            let s = next(&TERMINAL, c.terminal_notify);
            config::set("terminal_notify", &s.to_string());
            (here(main()), Some(Change::Terminal(s)))
        }
        "autostart" => {
            crate::autostart::set(!crate::autostart::is_enabled());
            (here(main()), None)
        }
        "city" => (Act::Push(city_screen()), None),
        "modules" => (Act::Push(modules_screen()), None),
        "about" => (Act::Push(about_screen()), None),
        "restart" => (Act::Restart, None),
        "config" => {
            open(&config::path().to_string_lossy());
            (Act::None, None)
        }
        "mod.folder" => {
            if let Some(d) = modules::dir() {
                open(&d.to_string_lossy());
            }
            (Act::None, None)
        }
        "about.source" => {
            open(SOURCE);
            (Act::None, None)
        }
        "city.auto" => {
            let auto = c.city.is_none() && !MANUAL_CITY.load(Ordering::Relaxed);
            if auto {
                // Switched off: wait for a name; touch the config once it is entered.
                MANUAL_CITY.store(true, Ordering::Relaxed);
                (Act::Replace { screen: city_screen(), root: None }, None)
            } else {
                MANUAL_CITY.store(false, Ordering::Relaxed);
                config::set("city", "");
                (Act::Replace { screen: city_screen(), root: Some(main()) }, Some(Change::City(None)))
            }
        }
        "city.name" => {
            let name = text.unwrap_or_default();
            if name.is_empty() {
                return (Act::None, None);
            }
            MANUAL_CITY.store(false, Ordering::Relaxed);
            config::set("city", &name);
            (Act::Replace { screen: city_screen(), root: Some(main()) }, Some(Change::City(Some(name))))
        }
        "discard" => {
            discard();
            (Act::Close, None)
        }
        // Installs the package in a separate process: it asks for UAC and, once done, tells the island to
        // start the module and refresh the settings.
        "notify.setup" => {
            if let Ok(exe) = std::env::current_exe()
                && let Err(e) = std::process::Command::new(exe).arg("notify-setup").spawn()
            {
                proto::error!("notify", "setup: {e}");
            }
            (Act::None, None)
        }
        "notify.banners" => {
            remember(&c);
            config::set("notify_banners", banners(!c.notify_ours));
            settle();
            (Act::Replace { screen: modules_screen(), root: Some(main()) }, None)
        }
        m if m.starts_with("mod.") => {
            remember(&c);
            let id = &m[4..];
            let packaged = modules::scan().iter().any(|m| m.id == id && m.package);
            modules::set_enabled(id, modules::off(&c).iter().any(|o| o == id));
            settle();
            // A packaged module switches after a restart, the rest right away.
            let change = (!packaged).then(|| Change::Module(id.to_string()));
            (Act::Replace { screen: modules_screen(), root: Some(main()) }, change)
        }
        _ => (Act::None, None),
    }
}

/// Opens a file, folder or link with the Windows default handler.
fn open(target: &str) {
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::HSTRING,
    };
    // SAFETY: the verb and target strings outlive the call; other pointers are null.
    unsafe {
        ShellExecuteW(None, windows::core::w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
    }
}
