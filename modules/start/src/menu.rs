// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Main screen layout and action handlers for the custom Start menu.

use std::{os::windows::process::CommandExt, process::Command};

use proto::{ModOut, Row, Screen};
use windows::{
    Win32::{
        System::{Power::SetSuspendState, Shutdown::LockWorkStation},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::{HSTRING, w},
};

use crate::{about, write};

/// Prevents console windows from flashing when executing commands.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Strings {
    pub about: &'static str,
    pub settings: &'static str,
    pub taskmgr: &'static str,
    pub sleep: &'static str,
    pub restart: &'static str,
    pub restart_q: &'static str,
    pub shutdown: &'static str,
    pub shutdown_q: &'static str,
    pub lock: &'static str,
    pub logout: &'static str,
    pub logout_q: &'static str,
}

const RU: Strings = Strings {
    about: "О системе",
    settings: "Параметры",
    taskmgr: "Диспетчер задач",
    sleep: "Спящий режим",
    restart: "Перезагрузка",
    restart_q: "Перезагрузить?",
    shutdown: "Завершение работы",
    shutdown_q: "Завершить работу?",
    lock: "Заблокировать",
    logout: "Выйти из системы",
    logout_q: "Выйти из системы?",
};

const EN: Strings = Strings {
    about: "About",
    settings: "Settings",
    taskmgr: "Task Manager",
    sleep: "Sleep",
    restart: "Restart",
    restart_q: "Restart?",
    shutdown: "Shut down",
    shutdown_q: "Shut down?",
    lock: "Lock",
    logout: "Sign out",
    logout_q: "Sign out?",
};

const ZH: Strings = Strings {
    about: "关于",
    settings: "设置",
    taskmgr: "任务管理器",
    sleep: "睡眠",
    restart: "重启",
    restart_q: "重启？",
    shutdown: "关机",
    shutdown_q: "关机？",
    lock: "锁定",
    logout: "注销",
    logout_q: "注销？",
};

const JA: Strings = Strings {
    about: "バージョン情報",
    settings: "設定",
    taskmgr: "タスク マネージャー",
    sleep: "スリープ",
    restart: "再起動",
    restart_q: "再起動しますか？",
    shutdown: "シャットダウン",
    shutdown_q: "シャットダウンしますか？",
    lock: "ロック",
    logout: "サインアウト",
    logout_q: "サインアウトしますか？",
};

pub fn strings() -> &'static Strings {
    match proto::language() {
        "ru" => &RU,
        "zh" => &ZH,
        "ja" => &JA,
        _ => &EN,
    }
}

fn item(id: &str, text: &str, keys: &str, confirm: Option<&str>) -> Row {
    Row::Item { id: id.into(), text: text.into(), hint: String::new(), keys: keys.into(), command: String::new(), confirm: confirm.map(String::from) }
}

pub fn main_screen() -> Screen {
    let t = strings();
    Screen {
        title: None,
        rows: vec![
            item("about", t.about, "", None),
            Row::Divider,
            item("settings", t.settings, "Win+I", None),
            item("taskmgr", t.taskmgr, "Ctrl+Shift+Esc", None),
            Row::Divider,
            item("sleep", t.sleep, "", None),
            item("restart", t.restart, "", Some(t.restart_q)),
            item("shutdown", t.shutdown, "", Some(t.shutdown_q)),
            Row::Divider,
            item("lock", t.lock, "Win+L", None),
            item("logout", t.logout, "", Some(t.logout_q)),
        ],
    }
}

/// Uses ShellExecute instead of Command::new: it opens URIs (`ms-settings:`) and shows the
/// elevation prompt for Task Manager.
fn shell(target: &str) {
    // SAFETY: the verb and target strings outlive the call; other pointers are null.
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
    }
}

fn shutdown(arg: &str) {
    let _ = Command::new("shutdown.exe").args([arg, "/t", "0"]).creation_flags(CREATE_NO_WINDOW).spawn();
}

/// Routes the selected item: "about" navigates to the next screen, others execute and close.
pub fn select(id: &str) {
    if id == "about" {
        return write(&ModOut::Push { screen: about::screen() });
    }
    write(&ModOut::Close);
    crate::MENU_OPEN.store(false, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: power and session calls take plain values and have no memory-safety preconditions.
    unsafe {
        match id {
            "settings" => shell("ms-settings:"),
            "taskmgr" => shell("taskmgr.exe"),
            "sleep" => {
                let _ = SetSuspendState(false, false, false);
            }
            "restart" => shutdown("/r"),
            "shutdown" => shutdown("/s"),
            "lock" => {
                let _ = LockWorkStation();
            }
            "logout" => shutdown("/l"),
            _ => {}
        }
    }
}
