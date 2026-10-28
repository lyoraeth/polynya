// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Modules: folders `%LOCALAPPDATA%\Polynya\modules\<id>\` with a `module.toml` manifest.
//!
//! The core finds modules by folder: deleting a module means deleting its folder. Disabled ones are
//! listed in the config (`modules_off`); experimental ones stay off until enabled (`modules_on`).
//! A module talks to the core only through the protocol in the `proto` crate.
//!
//! A module process is started as `exe [args] run` in its own folder and talks JSON lines over
//! stdin/stdout. A resident module starts with the core; any other module starts on the first
//! `invoke` of its launcher command and may exit when it is done. Its stderr goes to the log, and
//! an abnormal exit shows a chip, so a broken module is never silent.

use std::{
    collections::HashSet,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{ChildStdin, Command as Command_, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use proto::{Line, ModIn, ModOut, Msg};

/// A module process starts without a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// How long a stopped module may take to exit after its stdin closes before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(3);
/// A resident module that crashes after running at least this long is restarted once; one that
/// crashes sooner is broken and stays down until it is fixed and reloaded.
const RESTART_AFTER: Duration = Duration::from_secs(10);
/// A resident module exiting this soon with code 0 most likely forgot to keep reading stdin.
const EARLY_EXIT: Duration = Duration::from_secs(2);
/// Longest stdout line quoted in the log.
const LOG_QUOTE: usize = 200;

/// Modules shipped with the core: id, exe, manifest, and launch through the identity package
/// (see `package`). The latter is internal and unavailable to third-party modules.
///
/// `modules/start` is built but not shipped yet: it is installed by hand while it matures.
const BUNDLED: [(&str, &str, &str, bool); 3] = [
    ("agents", "polynya-agents.exe", include_str!("../modules/agents/module.toml"), false),
    ("notify", "polynya-notify.exe", include_str!("../modules/notify/module.toml"), true),
    ("picker", "polynya-picker.exe", include_str!("../modules/picker/module.toml"), false),
];

/// Built-in script modules: the files are embedded in the core, the interpreter is the user's.
/// Empty for now: `modules/devtools` is not shipped yet and is installed by hand while it matures.
const SCRIPTS: [(&str, &[(&str, &str)]); 0] = [];

#[derive(Debug)]
pub struct Manifest {
    pub id: String,
    /// The module's folder; also the working directory of its process.
    pub folder: PathBuf,
    /// `exe` as written: a file in the module folder (`main.py`, `tool.exe`) or a program in PATH.
    pub exe: String,
    /// Arguments placed before the core's own (`run`, `register`, ...).
    pub args: Vec<String>,
    /// Commands: search rows and hotkeys.
    pub commands: Vec<Command>,
    /// The module registers itself in external programs: the core calls `exe register` when it is
    /// enabled and `exe unregister` when disabled.
    pub hooks: bool,
    /// Resident: the core keeps it running from its own start.
    pub resident: bool,
    /// Works only while the island lives in the taskbar (e.g. Start).
    pub taskbar: bool,
    /// Resident, but launched through the identity package (see `package`): it has no stdin/stdout,
    /// and writes to the core over the pipe. Built-in only, see [`BUNDLED`].
    pub package: bool,
    /// Off until enabled (`modules_on`).
    pub experimental: bool,
    /// Name in the interface language, for the settings.
    pub name: String,
    /// The manifest text: a changed manifest restarts the module.
    source: String,
}

impl Manifest {
    /// Path of `exe` inside the module folder.
    pub fn exe_path(&self) -> PathBuf {
        self.folder.join(&self.exe)
    }

    /// Program to start and the arguments before the core's own. Scripts run through their
    /// interpreter, so `exe = "main.py"` or `exe = "main.js"` just works.
    fn launch(&self) -> Result<(PathBuf, Vec<String>), String> {
        let local = self.exe_path();
        let file = || local.display().to_string();
        let ext = Path::new(&self.exe).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let script = matches!(ext.as_str(), "py" | "pyw" | "js" | "mjs" | "cjs" | "ps1");
        if script && !local.is_file() {
            return Err(format!("{} not found in the module folder", self.exe));
        }
        let (program, mut args) = match ext.as_str() {
            // The `py` launcher comes with python.org installs; `python` with the Store and conda.
            "py" | "pyw" => match which("py") {
                Some(py) => (py, vec!["-3".into(), file()]),
                None => (which("python").ok_or("Python is not installed or not in PATH")?, vec![file()]),
            },
            "js" | "mjs" | "cjs" => (which("node").ok_or("Node.js is not installed or not in PATH")?, vec![file()]),
            "ps1" => (
                which("powershell").ok_or("PowerShell not found")?,
                ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].map(String::from).into_iter().chain([file()]).collect(),
            ),
            _ if local.is_file() => (local, Vec::new()),
            _ => (which(&self.exe).ok_or_else(|| format!("exe \"{}\" is neither in the module folder nor in PATH", self.exe))?, Vec::new()),
        };
        args.extend(self.args.iter().cloned());
        Ok((program, args))
    }

    /// The interpreter this script module needs but the system lacks (`Node.js`, `Python`,
    /// `PowerShell`); `None` if it can run. Such a module stays out of search and hotkeys and is
    /// not started: every attempt would only fail.
    pub fn missing(&self) -> Option<&'static str> {
        let ext = Path::new(&self.exe).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "py" | "pyw" if which("py").is_none() && which("python").is_none() => Some("Python"),
            "js" | "mjs" | "cjs" if which("node").is_none() => Some("Node.js"),
            "ps1" if which("powershell").is_none() => Some("PowerShell"),
            _ => None,
        }
    }

    /// A command for `exe [args] <arg>` in the module folder, with the module environment.
    fn command(&self, arg: &str) -> Result<Command_, String> {
        let (program, args) = self.launch()?;
        let mut c = Command_::new(program);
        c.args(args).arg(arg).current_dir(&self.folder).creation_flags(CREATE_NO_WINDOW);
        c.env("POLYNYA_MODULE", &self.id).env("POLYNYA_LANG", proto::language()).env("POLYNYA_API", API.to_string());
        c.env("POLYNYA_VERSION", env!("POLYNYA_VERSION"));
        // Python block-buffers a pipe and writes it in the ANSI code page: replies would get stuck in
        // the buffer and non-Latin text would break.
        c.env("PYTHONUNBUFFERED", "1").env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
        Ok(c)
    }
}

/// Finds a program in PATH, trying the PATHEXT extensions.
fn which(name: &str) -> Option<PathBuf> {
    let exts = [".exe", ".cmd", ".bat", ".com"];
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        let plain = dir.join(name);
        if Path::new(name).extension().is_some() && plain.is_file() {
            return Some(plain);
        }
        exts.iter().map(|e| dir.join(format!("{name}{e}"))).find(|p| p.is_file())
    })
}

/// Modules folder next to the core exe: in its permanent place, in a portable copy next to it.
pub fn dir() -> Option<PathBuf> {
    match proto::portable_root() {
        Some(root) => Some(root.join("modules")),
        None => crate::install::path()?.parent().map(|d| d.join("modules")),
    }
}

/// Manifest and protocol version the core understands: a module with a newer `api` is not loaded.
/// Without `api` it is 1.
const API: i64 = 1;

/// The manifest is TOML. Texts (`name`, `title`, `hint`) are a string or per language:
/// `name.ru = "…"`, `name.en = "…"`; the interface language is used, else English.
fn parse(text: &str, folder: &Path) -> Result<Manifest, String> {
    let source = text.to_string();
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
    let str_ = |k: &str| t.get(k).and_then(|v| v.as_str());
    let flag = |k: &str| t.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let api = t.get("api").and_then(|v| v.as_integer()).unwrap_or(1);
    if api > API {
        return Err(format!("api = {api}, supported up to {API}"));
    }
    // Where the module plugs in: search rows, right-zone chips or nowhere in particular. The core
    // does not need to tell them apart yet, but an unknown value must not pass.
    if let Some(kind) = str_("kind")
        && !matches!(kind, "search" | "zone" | "other")
    {
        return Err(format!("kind = {kind}, expected search, zone or other"));
    }
    let lang = proto::language();
    let text = |k: &str| localized(&t, k, lang);
    let id = str_("id").ok_or("no id")?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("id = \"{id}\": use only latin letters, digits, - and _"));
    }
    let exe = str_("exe").ok_or("no exe")?;
    let args = match t.get("args") {
        None => Vec::new(),
        Some(toml::Value::Array(a)) => a.iter().map(|v| v.as_str().map(String::from).ok_or("args must be strings")).collect::<Result<_, _>>()?,
        Some(_) => return Err("args must be an array of strings".into()),
    };
    // The old layout would load silently without commands: say what changed.
    if t.contains_key("command") || t.contains_key("commands") {
        return Err("command/[[commands]] were replaced by [main] and [[functions]], see the module API".into());
    }
    let name = text("name").unwrap_or_else(|| id.to_string());
    let mut commands = Vec::new();
    match t.get("main") {
        Some(toml::Value::Table(m)) => commands.push(command(m, lang, MAIN, &name, None)?),
        Some(_) => return Err("main must be a table ([main])".into()),
        None => {}
    }
    match t.get("functions") {
        Some(toml::Value::Array(a)) => {
            for v in a {
                let f = v.as_table().ok_or("functions must be tables ([[functions]])")?;
                let fid = f.get("id").and_then(|v| v.as_str()).ok_or("a function without an id")?;
                if fid == MAIN {
                    return Err(format!("function id \"{MAIN}\" is reserved for [main]"));
                }
                if commands.iter().any(|c: &Command| c.id == fid) {
                    return Err(format!("function \"{fid}\" is declared twice"));
                }
                commands.push(command(f, lang, fid, &name, Some(&name))?);
            }
        }
        Some(_) => return Err("functions must be an array of tables ([[functions]])".into()),
        None => {}
    }
    Ok(Manifest {
        name,
        id: id.to_string(),
        folder: folder.to_path_buf(),
        exe: exe.to_string(),
        args,
        commands,
        hooks: flag("hooks"),
        resident: flag("resident"),
        taskbar: flag("taskbar"),
        experimental: flag("experimental"),
        // The package is internal: built-in modules only, not exposed in the manifest. The folder
        // must be the built-in one too, so a third-party module reusing the id cannot take it.
        package: BUNDLED.iter().any(|b| b.0 == id && b.3) && folder.file_name().is_some_and(|f| f == id),
        source,
    })
}

/// The command that opens the module's main interface: `[main]` in the manifest.
pub const MAIN: &str = "main";

/// Something a module does on request: its main interface (`[main]`) or one of its functions
/// (`[[functions]]`). Shown in search, bound to a hotkey, and run with `invoke`. The core matches
/// input against the title and keywords itself; the module is not running until it is chosen.
#[derive(Debug, Clone)]
pub struct Command {
    /// `main` or the function id.
    pub id: String,
    /// For `main` the module name; for a function its name as in the module's menu.
    pub title: String,
    /// A function's module name, shown before the title in search: "Devtools › Convert value".
    pub prefix: Option<String>,
    pub hint: String,
    pub keywords: Vec<String>,
    /// SVG path on a 24×24 grid, see `icons::draw_path`.
    pub icon: String,
    /// Global hotkey as written in the manifest (`Ctrl+Alt+D`); `config.ini` can override it.
    pub hotkey: Option<String>,
    /// On a hotkey the core copies the selection of the foreground app and sends it in `invoke`.
    pub selection: bool,
    /// Shown in search; a function can opt out and stay reachable by its hotkey and the menu.
    pub search: bool,
}

/// A manifest string: plain, or a table by language (`title.ru = "…"`) falling back to English.
fn localized(t: &toml::Table, k: &str, lang: &str) -> Option<String> {
    match t.get(k)? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(by_lang) => by_lang.get(lang).or_else(|| by_lang.get("en")).and_then(|v| v.as_str()).map(String::from),
        _ => None,
    }
}

/// Reads `[main]` or a `[[functions]]` entry. `prefix` is the module name for a function.
fn command(t: &toml::Table, lang: &str, id: &str, name: &str, prefix: Option<&str>) -> Result<Command, String> {
    let str_ = |k: &str| t.get(k).and_then(|v| v.as_str());
    let text = |k: &str| localized(t, k, lang);
    let title = match prefix {
        Some(_) => text("title").ok_or_else(|| format!("function \"{id}\" has no title: use its name from the module's menu"))?,
        None => name.to_string(),
    };
    Ok(Command {
        id: id.to_string(),
        title,
        prefix: prefix.map(String::from),
        hint: text("hint").unwrap_or_default(),
        keywords: t
            .get("keywords")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|k| k.as_str()).map(str::to_lowercase).collect())
            .unwrap_or_default(),
        icon: str_("icon").unwrap_or_default().to_string(),
        hotkey: str_("hotkey").filter(|h| !h.is_empty()).map(String::from),
        selection: t.get("selection").and_then(|v| v.as_bool()).unwrap_or(false),
        search: t.get("search").and_then(|v| v.as_bool()).unwrap_or(true),
    })
}

/// Commands of enabled modules, with the id of the module that runs each.
pub fn commands(off: &[String]) -> Vec<(String, Command)> {
    scan().into_iter().filter(|m| !off.contains(&m.id) && m.missing().is_none()).flat_map(|m| m.commands.into_iter().map(move |c| (m.id.clone(), c))).collect()
}

/// Hotkeys of enabled modules' commands: the manifest's, overridden by `config.ini`.
pub fn hotkeys(off: &[String], overrides: &[(String, String)]) -> Vec<(String, crate::hotkeys::Bound)> {
    commands(off)
        .into_iter()
        .filter_map(|(module, c)| {
            let key = format!("{module}.{}", c.id);
            let keys = match overrides.iter().rfind(|(k, _)| *k == key) {
                Some((_, keys)) => keys.clone(),
                None => c.hotkey?,
            };
            (!keys.is_empty()).then_some((keys, crate::hotkeys::Bound { module, command: c.id, selection: c.selection }))
        })
        .collect()
}

/// Disabled modules: those in `modules_off` and experimental ones missing from `modules_on`.
pub fn off(c: &crate::config::Config) -> Vec<String> {
    let mut off = c.modules_off.clone();
    off.extend(scan().into_iter().filter(|m| m.experimental && !c.modules_on.contains(&m.id)).map(|m| m.id));
    off
}

/// Turns a module on or off in the config. An experimental module is off unless listed in
/// `modules_on`, the rest are on unless listed in `modules_off`.
pub fn set_enabled(id: &str, on: bool) {
    let c = crate::config::load();
    let experimental = scan().iter().any(|m| m.id == id && m.experimental);
    let (key, mut list, listed) = if experimental { ("modules_on", c.modules_on, on) } else { ("modules_off", c.modules_off, !on) };
    list.retain(|o| o != id);
    if listed {
        list.push(id.to_string());
    }
    crate::config::set(key, &list.join(" "));
}

/// Problems already reported, so a rescan does not repeat them.
static REPORTED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Logs a problem with a module folder once and shows it as a chip: a module author sees why the
/// module is missing without digging in the log.
fn report(folder: &Path, problem: &str) {
    let what = format!("{}: {problem}", folder.display());
    let new = REPORTED.lock().map(|mut r| r.get_or_insert_with(HashSet::new).insert(what.clone())).unwrap_or(false);
    if new {
        proto::warn!("modules", "{what}");
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        crate::ipc::inject(failure(&name, &name, &format!("module.toml: {problem}")));
    }
}

/// Installed modules. A folder without a valid manifest is reported once and skipped.
pub fn scan() -> Vec<Manifest> {
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Manifest> = Vec::new();
    for folder in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(text) = fs::read_to_string(folder.join("module.toml")) else {
            report(&folder, "no module.toml in the folder");
            continue;
        };
        match parse(&text, &folder) {
            Ok(m) if out.iter().any(|o| o.id == m.id) => report(&folder, &format!("id \"{}\" is already used by another module", m.id)),
            Ok(m) => out.push(m),
            Err(e) => report(&folder, &e),
        }
    }
    out
}

/// Calls `register` for enabled modules with hooks and `unregister` for disabled ones. Does not
/// wait for a reply: the call is idempotent, and the module knows where to register itself.
///
/// A disabled module's package is removed, and the module is told to undo its system changes.
pub fn sync(off: &[String]) {
    for m in scan().into_iter().filter(|m| m.hooks) {
        hooks(&m, !off.contains(&m.id));
    }
    for m in scan().into_iter().filter(|m| m.package && off.contains(&m.id)) {
        if crate::package::state(&m.folder) != crate::package::State::Missing {
            if let Err(e) = crate::package::remove() {
                proto::error!("modules", "{}: remove package: {e}", m.id);
            }
            restore(&m);
        }
    }
}

/// Undoes the system changes of every module and waits for it: hooks are unregistered, the
/// package is removed and the module restores what it changed. For `polynya uninstall`, right
/// before the files go away, so nothing may stay running in the background.
pub fn teardown() {
    for m in scan() {
        if m.hooks {
            wait(&m, "unregister");
        }
        if m.package && crate::package::state(&m.folder) != crate::package::State::Missing {
            if let Err(e) = crate::package::remove() {
                proto::error!("modules", "{}: remove package: {e}", m.id);
            }
            wait(&m, "restore");
        }
    }
}

/// Runs a lifecycle command of a module and waits for it; failures go to the log.
fn wait(m: &Manifest, arg: &str) {
    match m.command(arg).and_then(|mut c| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| e.to_string())) {
        Ok(s) if s.success() => {}
        Ok(s) => proto::error!("modules", "{}: {arg} failed ({s})", m.id),
        Err(e) => proto::error!("modules", "{}: {arg}: {e}", m.id),
    }
}

/// Runs `exe register` or `exe unregister` without waiting.
fn hooks(m: &Manifest, on: bool) {
    let arg = if on { "register" } else { "unregister" };
    detached(m, arg);
}

/// Has the module undo its system changes (`exe restore`): it is not running or has crashed.
fn restore(m: &Manifest) {
    detached(m, "restore");
}

/// Runs a one-off lifecycle command of a module; failures and stderr go to the log.
fn detached(m: &Manifest, arg: &'static str) {
    let spawned = m.command(arg).and_then(|mut c| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string()));
    match spawned {
        Ok(mut child) => {
            let id = m.id.clone();
            let _ = std::thread::Builder::new().name(format!("module-{id}-{arg}")).spawn(move || {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                match child.wait() {
                    Ok(s) if s.success() => {}
                    Ok(s) => proto::error!("modules", "{id}: {arg} failed ({s}): {}", err.trim()),
                    Err(e) => proto::error!("modules", "{id}: {arg}: {e}"),
                }
            });
        }
        Err(e) => proto::error!("modules", "{}: {arg}: {e}", m.id),
    }
}

/// Running packaged modules, so they are not started twice.
static PACKAGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Starts enabled packaged modules through the package. Without the package installed the module
/// waits for `polynya notify-setup`. The core watches the process: on a crash it is started once more
/// (and undoes its own system changes); on a second crash the core undoes them for it, and the
/// module stays off until the island restarts.
pub fn start_packaged(off: &[String]) {
    for m in scan().into_iter().filter(|m| m.package && m.resident && !off.contains(&m.id)) {
        let crate::package::State::Ready(aumid) = crate::package::state(&m.folder) else {
            proto::warn!("modules", "{}: package not installed, run polynya notify-setup", m.id);
            continue;
        };
        {
            let Ok(mut running) = PACKAGED.lock() else { continue };
            if running.contains(&m.id) {
                continue;
            }
            running.push(m.id.clone());
        }
        launch_packaged(m, aumid, true);
    }
}

fn launch_packaged(m: Manifest, aumid: String, retry: bool) {
    let pid = match crate::package::launch(&aumid, &format!("run {}", std::process::id())) {
        Ok(pid) => pid,
        Err(e) => {
            proto::error!("modules", "{}: launch via package: {e}", m.id);
            PACKAGED.lock().map(|mut r| r.retain(|id| id != &m.id)).ok();
            // This may have been a restart after a crash: undo what the module changed.
            restore(&m);
            return;
        }
    };
    let _ = std::thread::Builder::new().name(format!("module-{}", m.id)).spawn(move || {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        // SAFETY: `OpenProcess` has no memory-safety preconditions; `wait` closes the returned handle.
        if let Ok(h) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
            crate::package::wait(h);
        }
        proto::warn!("modules", "{} exited{}", m.id, if retry { ", restarting" } else { "" });
        // A restarted module starts with the rollback itself; a parallel one is not needed.
        if retry {
            launch_packaged(m, aumid, false);
        } else {
            restore(&m);
            PACKAGED.lock().map(|mut r| r.retain(|id| id != &m.id)).ok();
        }
    });
}

// ── Module processes ──

/// A running module process.
struct Proc {
    id: String,
    pid: u32,
    stdin: ChildStdin,
    /// Set when the core stops the process itself, so its exit is not a crash.
    stopping: Arc<AtomicBool>,
    /// The manifest it was started with.
    source: String,
}

/// Running module processes.
static PROCS: Mutex<Vec<Proc>> = Mutex::new(Vec::new());
/// Held while a module is being started: a run from search and a restart after a crash must not
/// start two processes of one module.
static SPAWNING: Mutex<()> = Mutex::new(());
/// Messages from module processes: module id and message.
static INBOX: Mutex<Vec<(String, ModOut)>> = Mutex::new(Vec::new());
/// Wakes the UI thread when a module message arrives.
static WAKE: OnceLock<fn()> = OnceLock::new();

/// Sets how module threads wake the UI thread. Call once before starting modules.
pub fn init(wake: fn()) {
    let _ = WAKE.set(wake);
}

fn wake() {
    if let Some(w) = WAKE.get() {
        w();
    }
}

/// A crash chip: `key` per module, so a repeated failure replaces the previous one.
fn failure(id: &str, name: &str, reason: &str) -> Msg {
    let tr = crate::i18n::t();
    let reason: String = reason.trim().chars().take(LOG_QUOTE).collect();
    Msg::Chip {
        key: format!("module:{id}"),
        source: None,
        icon: "fail".into(),
        title: format!("{name} · {}", tr.module_stopped),
        body: reason,
        meta: "Polynya".into(),
        ttl_ms: Some(15_000),
        animated: false,
        app: None,
        popup: false,
        swatches: Vec::new(),
        mark: None,
    }
}

fn running(id: &str) -> bool {
    PROCS.lock().is_ok_and(|p| p.iter().any(|p| p.id == id))
}

/// Starts the module process (`exe [args] run`), with readers for its stdout and stderr and a
/// watcher for its exit.
fn spawn(m: &Manifest) -> Result<u32, String> {
    let _starting = SPAWNING.lock();
    if let Some(pid) = PROCS.lock().ok().and_then(|p| p.iter().find(|p| p.id == m.id).map(|p| p.pid)) {
        return Ok(pid);
    }
    let mut child = m.command("run")?.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        return Err("no stdio pipes".into());
    };
    let pid = child.id();
    let stopping = Arc::new(AtomicBool::new(false));
    if let Ok(mut p) = PROCS.lock() {
        p.push(Proc { id: m.id.clone(), pid, stdin, stopping: stopping.clone(), source: m.source.clone() });
    }
    proto::info!("modules", "{} started (pid {pid})", m.id);

    let id = m.id.clone();
    let _ = std::thread::Builder::new().name(format!("module-{id}-out")).spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        // Raw bytes, not `lines()`: one line that is not UTF-8 must not end the reading.
        while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
            let line = String::from_utf8_lossy(&buf).trim().to_string();
            buf.clear();
            if line.is_empty() {
                continue;
            }
            match proto::parse_line(&line) {
                Ok(Line::Out(msg)) => {
                    if let Ok(mut q) = INBOX.lock() {
                        q.push((id.clone(), msg));
                    }
                }
                Ok(Line::Msg(msg)) => crate::ipc::inject(msg),
                Err(e) => {
                    let quote: String = line.chars().take(LOG_QUOTE).collect();
                    proto::warn!("modules", "{id}: bad stdout line ({e}): {quote}");
                    continue;
                }
            }
            wake();
        }
    });

    let last_err = Arc::new(Mutex::new(String::new()));
    let (id, last) = (m.id.clone(), last_err.clone());
    let err_reader = std::thread::Builder::new().name(format!("module-{id}-err")).spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
            let line = String::from_utf8_lossy(&buf).trim_end().to_string();
            buf.clear();
            if !line.trim().is_empty() {
                proto::warn!("modules", "{id}: {line}");
                if let Ok(mut l) = last.lock() {
                    *l = line;
                }
            }
        }
    });

    let (id, name, resident, started) = (m.id.clone(), m.name.clone(), m.resident, Instant::now());
    let _ = std::thread::Builder::new().name(format!("module-{id}")).spawn(move || {
        let status = child.wait();
        // The stderr reader ends with the process; wait for it to have the last line.
        if let Ok(r) = err_reader {
            let _ = r.join();
        }
        if let Ok(mut p) = PROCS.lock() {
            p.retain(|p| p.pid != pid);
        }
        // A dropdown of a gone module would answer nothing.
        if let Ok(mut q) = INBOX.lock() {
            q.push((id.clone(), ModOut::Close));
        }
        wake();
        if stopping.load(Ordering::Relaxed) {
            proto::info!("modules", "{id} stopped");
            return;
        }
        let ran = started.elapsed();
        let code = status.as_ref().ok().and_then(|s| s.code());
        if code == Some(0) {
            if resident && ran < EARLY_EXIT {
                proto::warn!("modules", "{id} exited right after start; a resident module must keep reading stdin");
            } else {
                proto::info!("modules", "{id} exited");
            }
            return;
        }
        let last = last_err.lock().map(|l| l.clone()).unwrap_or_default();
        let reason = if last.is_empty() { code.map_or("terminated".to_string(), |c| format!("exit code {c}")) } else { last };
        proto::error!("modules", "{id} crashed ({}) after {} s: {reason}", code.map_or("killed".into(), |c| c.to_string()), ran.as_secs());
        crate::ipc::inject(failure(&id, &name, &reason));
        wake();
        if resident && ran >= RESTART_AFTER {
            std::thread::sleep(Duration::from_secs(1));
            if let Some(m) = scan().into_iter().find(|m| m.id == id) {
                proto::info!("modules", "{id}: restarting after a crash");
                if let Err(e) = spawn(&m) {
                    proto::error!("modules", "{id}: restart: {e}");
                }
            }
        }
    });
    Ok(pid)
}

/// Stops a module process: closes its stdin (the signal to exit) and kills it if it is still alive
/// after [`STOP_GRACE`].
fn stop(id: &str) {
    let Some(p) = PROCS.lock().ok().and_then(|mut procs| procs.iter().position(|p| p.id == id).map(|i| procs.remove(i))) else { return };
    p.stopping.store(true, Ordering::Relaxed);
    let pid = p.pid;
    drop(p.stdin);
    let _ = std::thread::Builder::new().name(format!("module-{id}-stop")).spawn(move || {
        use windows::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject},
        };
        // SAFETY: `h` is opened here and closed exactly once; a pid that already exited only makes
        // `OpenProcess` fail.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid) else { return };
            if WaitForSingleObject(h, STOP_GRACE.as_millis() as u32) == WAIT_TIMEOUT {
                let _ = TerminateProcess(h, 1);
            }
            let _ = CloseHandle(h);
        }
    });
}

/// Brings running module processes in line with the installed and enabled modules: stops those that
/// are gone, disabled, changed or taskbar-only on a vertical taskbar, and starts enabled resident
/// modules that are not running. Cheap enough to call on every launcher open.
pub fn reconcile(off: &[String], vertical: bool) {
    let all = scan();
    let wanted = |m: &Manifest| !off.contains(&m.id) && !(vertical && m.taskbar) && !m.package && m.missing().is_none();
    let stale: Vec<String> = PROCS
        .lock()
        .map(|procs| procs.iter().filter(|p| !all.iter().any(|m| m.id == p.id && m.source == p.source && wanted(m))).map(|p| p.id.clone()).collect())
        .unwrap_or_default();
    for id in stale {
        proto::info!("modules", "{id}: stopping (disabled, removed or changed)");
        stop(&id);
    }
    for m in all.iter().filter(|m| m.resident && wanted(m) && !running(&m.id)) {
        if let Err(e) = spawn(m) {
            proto::error!("modules", "{}: start: {e}", m.id);
            crate::ipc::inject(failure(&m.id, &m.name, &e));
        }
    }
}

/// Restarts every module process: for a module author after changing code.
pub fn reload(off: &[String], vertical: bool) {
    let ids: Vec<String> = PROCS.lock().map(|p| p.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
    for id in ids {
        stop(&id);
    }
    if let Ok(mut r) = REPORTED.lock() {
        *r = None;
    }
    reconcile(off, vertical);
}

/// A module was switched on or off in the settings: registers or removes its hooks and starts or
/// stops its process.
pub fn toggled(id: &str, off: &[String], vertical: bool) {
    if let Some(m) = scan().into_iter().find(|m| m.id == id && m.hooks) {
        hooks(&m, !off.contains(&m.id));
    }
    reconcile(off, vertical);
}

/// Runs a module command (`main` or a function) from `source`: starts the module if it is not
/// running and sends it `invoke` with the clipboard text. The module is allowed to bring its own
/// window to the foreground.
pub fn invoke(id: &str, command: &str, source: proto::Source, query: &str, selection: String) -> Result<(), String> {
    let m = scan().into_iter().find(|m| m.id == id).ok_or_else(|| format!("module {id} not found"))?;
    if !running(id)
        && let Err(e) = spawn(&m)
    {
        // Seen at once: the user pressed something and nothing happened otherwise.
        crate::ipc::inject(failure(&m.id, &m.name, &e));
        return Err(e);
    }
    let clipboard = crate::launcher::clipboard_text().unwrap_or_default();
    send(id, &ModIn::Invoke { command: command.into(), source, query: query.into(), selection, clipboard });
    let pid = PROCS.lock().ok().and_then(|p| p.iter().find(|p| p.id == id).map(|p| p.pid));
    if let Some(pid) = pid {
        // SAFETY: `AllowSetForegroundWindow` takes a plain pid and has no memory-safety preconditions.
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) };
    }
    Ok(())
}

/// Takes messages from module processes.
pub fn drain() -> Vec<(String, ModOut)> {
    INBOX.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// Sends a message to a running module process.
pub fn send(id: &str, msg: &ModIn) {
    let Ok(line) = serde_json::to_string(msg) else { return };
    if let Ok(mut procs) = PROCS.lock()
        && let Some(p) = procs.iter_mut().find(|p| p.id == id)
        && let Err(e) = writeln!(p.stdin, "{line}").and_then(|()| p.stdin.flush())
    {
        proto::error!("modules", "{id}: send: {e}");
    }
}

/// Copies modules built next to the core exe (`from` is its folder) into the modules folder.
/// Until there is an installer, this is how modules arrive with a new core version.
pub fn bundle(from: &Path) -> io::Result<()> {
    let Some(dir) = dir() else { return Ok(()) };
    for (id, exe, manifest, _) in BUNDLED {
        // The core was built without this module.
        let Ok(bytes) = fs::read(from.join(exe)) else { continue };
        let folder = dir.join(id);
        fs::create_dir_all(&folder)?;
        let dst = folder.join(exe);
        let old = dst.with_extension("old");
        let _ = fs::remove_file(&old);
        if fs::read(&dst).is_ok_and(|b| b == bytes) && fs::read_to_string(folder.join("module.toml")).is_ok_and(|m| m == manifest) {
            continue;
        }
        // The module exe may be running right now (an agent hook): it cannot be overwritten, but it can
        // be renamed.
        if dst.exists() {
            fs::rename(&dst, &old)?;
        }
        fs::write(&dst, bytes)?;
        fs::write(folder.join("module.toml"), manifest)?;
    }
    for (id, files) in SCRIPTS {
        let folder = dir.join(id);
        fs::create_dir_all(&folder)?;
        for (name, text) in files {
            let dst = folder.join(name);
            if fs::read_to_string(&dst).is_ok_and(|t| t == *text) {
                continue;
            }
            fs::write(dst, text)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifests_parse() {
        for (id, exe, manifest, package) in BUNDLED {
            let folder = Path::new("modules").join(id);
            let m = parse(manifest, &folder).unwrap();
            assert_eq!((m.id.as_str(), m.exe_path(), m.package), (id, folder.join(exe), package));
            // the same id in another folder does not get the package
            assert!(!parse(manifest, Path::new("m")).unwrap().package);
        }
        for (id, files) in SCRIPTS {
            let m = parse(files[0].1, Path::new("m")).unwrap();
            assert_eq!(m.id, id);
            assert!(files.iter().any(|(name, _)| *name == m.exe));
        }
    }

    #[test]
    fn third_party_manifest() {
        let m = parse(
            "id = \"x\"\nexe = \"x.exe\"\nname.en = \"Tool\"\npackage = true\n[main]\nkeywords = [\"Run\", \"start\"]\n[[functions]]\nid = \"go\"\ntitle.en = \"Go\"\nhotkey = \"Ctrl+Alt+G\"\n",
            Path::new("m"),
        )
        .unwrap();
        assert_eq!((m.commands[0].id.as_str(), m.commands[0].title.as_str()), (MAIN, "Tool"));
        assert_eq!(m.commands[0].keywords, ["run", "start"]);
        assert_eq!((m.commands[1].id.as_str(), m.commands[1].prefix.as_deref()), ("go", Some("Tool")));
        // The old layout, a function without a title and a function named like [main] are refused.
        assert!(parse("id = \"x\"\nexe = \"x.exe\"\ncommand = \"go\"\n", Path::new("m")).is_err());
        assert!(parse("id = \"x\"\nexe = \"x.exe\"\n[[functions]]\nid = \"go\"\n", Path::new("m")).is_err());
        assert!(parse("id = \"x\"\nexe = \"x.exe\"\n[[functions]]\nid = \"main\"\ntitle = \"M\"\n", Path::new("m")).is_err());
        // The package is built-in only, even if a third-party module asks for it.
        assert!(!m.package);
        assert!(parse("id = \"x\"\nexe = \"x.exe\"\napi = 2\n", Path::new("m")).is_err());
        assert!(parse("id = \"x\"\nexe = \"x.exe\"\nkind = \"tray\"\n", Path::new("m")).is_err());
        assert!(parse("id = \"a b\"\nexe = \"x.exe\"\n", Path::new("m")).is_err());
        let m = parse("id = \"x\"\nexe = \"node\"\nargs = [\"main.js\", \"--fast\"]\n", Path::new("m")).unwrap();
        assert_eq!(m.args, ["main.js", "--fast"]);
        assert!(parse("id = \"x\"\nexe = \"node\"\nargs = \"main.js\"\n", Path::new("m")).is_err());
    }
}
