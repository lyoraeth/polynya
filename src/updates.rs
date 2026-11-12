// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable release checks against GitHub, on a timer and on demand from settings.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use windows::core::{Error, Result};

use crate::{http, icons::Icon, island::zone::Note};

const ENDPOINT: &str = "/repos/lyoraeth/polynya/releases/latest";
const START_DELAY: Duration = Duration::from_secs(60);
const REFRESH: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_BODY: usize = 1024 * 1024;
pub const NOTICE_KEY: &str = "core:update";
pub const NOTICE_TTL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Never,
    Checking,
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    number: [u32; 3],
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub phase: Phase,
    pub available: Option<Release>,
}

#[derive(Debug)]
struct Shared {
    snapshot: Snapshot,
    notified: Option<String>,
    notice: Option<Release>,
}

impl Shared {
    const fn new() -> Self {
        Self { snapshot: Snapshot { phase: Phase::Never, available: None }, notified: None, notice: None }
    }
}

#[derive(Debug, Clone, Copy)]
enum Origin {
    Automatic,
    Manual,
}

enum Request {
    Check,
    NoticeShown(String),
}

impl Origin {
    fn name(self) -> &'static str {
        match self {
            Origin::Automatic => "automatic",
            Origin::Manual => "manual",
        }
    }
}

static STATE: Mutex<Shared> = Mutex::new(Shared::new());
static REQUEST: Mutex<Option<std::sync::mpsc::Sender<Request>>> = Mutex::new(None);

pub fn snapshot() -> Snapshot {
    STATE.lock().map(|s| s.snapshot.clone()).unwrap_or(Snapshot { phase: Phase::Failed, available: None })
}

pub fn check_now() {
    proto::info!("updates", "manual check requested");
    {
        let Ok(mut state) = STATE.lock() else { return };
        if state.snapshot.phase == Phase::Checking {
            proto::debug!("updates", "manual check joined the request already in flight");
            return;
        }
        state.snapshot.phase = Phase::Checking;
    }
    let sent = REQUEST.lock().ok().and_then(|r| r.clone()).is_some_and(|tx| tx.send(Request::Check).is_ok());
    if !sent {
        proto::error!("updates", "manual check failed: worker is unavailable");
        if let Ok(mut state) = STATE.lock() {
            state.snapshot.phase = Phase::Failed;
        }
    }
}

pub fn start(notify: fn()) {
    if let Ok(mut state) = STATE.lock() {
        state.notified = load_notified();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(mut request) = REQUEST.lock() {
        *request = Some(tx);
    }
    let spawned = std::thread::Builder::new().name("updates".into()).spawn(move || {
        let mut deadline = Instant::now() + START_DELAY;
        loop {
            let origin = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Request::Check) => Origin::Manual,
                Ok(Request::NoticeShown(version)) => {
                    save_notified(&version);
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Origin::Automatic,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            };
            deadline = Instant::now() + check(origin, notify);
        }
    });
    if let Err(e) = spawned {
        proto::error!("updates", "failed to start worker: {e}");
        if let Ok(mut state) = STATE.lock() {
            state.snapshot.phase = Phase::Failed;
        }
    }
}

pub fn pending_notice() -> Option<(String, Note)> {
    let release = STATE.lock().ok()?.notice.clone()?;
    let version = release.version.clone();
    let tr = crate::i18n::t();
    Some((
        version,
        Note {
            key: NOTICE_KEY.into(),
            source: Some("Polynya".into()),
            app: None,
            link: Some(release.url),
            image: None,
            popup: false,
            icon: Icon::Info,
            title: tr.update_available.into(),
            body: format!("{} {}", tr.version, release.version),
            meta: "GitHub".into(),
            ttl: None,
            animated: false,
            swatches: Vec::new(),
            mark: None,
        },
    ))
}

pub fn notice_shown(version: &str) {
    {
        let Ok(mut state) = STATE.lock() else { return };
        if !state.notice.as_ref().is_some_and(|release| release.version == version) {
            return;
        }
        state.notice = None;
        state.notified = Some(version.into());
    }
    let sent = REQUEST.lock().ok().and_then(|r| r.clone()).is_some_and(|tx| tx.send(Request::NoticeShown(version.into())).is_ok());
    if !sent {
        proto::warn!("updates", "could not save shown notification: worker is unavailable");
    }
}

fn check(origin: Origin, notify: fn()) -> Duration {
    if let Ok(mut state) = STATE.lock() {
        state.snapshot.phase = Phase::Checking;
    }
    notify();
    proto::info!("updates", "{} check: GET https://api.github.com{ENDPOINT}", origin.name());
    match fetch() {
        Ok(release) => {
            let current = match parse_version(env!("POLYNYA_VERSION")) {
                Some(current) => current,
                None => {
                    let error = format!("unsupported current version: {}", env!("POLYNYA_VERSION"));
                    proto::error!("updates", "{} check failed: {error}", origin.name());
                    finish_failure(notify);
                    return RETRY;
                }
            };
            let available = release.number > current;
            proto::info!(
                "updates",
                "{} check succeeded: current {}, latest {}{}",
                origin.name(),
                env!("POLYNYA_VERSION"),
                release.version,
                if available { " (update available)" } else { " (up to date)" }
            );
            if let Ok(mut state) = STATE.lock() {
                state.snapshot.phase = Phase::Success;
                state.snapshot.available = available.then_some(release.clone());
                if should_notify(current, &release, state.notified.clone()) {
                    state.notice = Some(release.clone());
                    proto::info!("updates", "update notification queued: {}", release.version);
                } else {
                    state.notice = None;
                }
            }
        }
        Err(e) => {
            proto::error!("updates", "{} check failed: {e}", origin.name());
            finish_failure(notify);
            return RETRY;
        }
    }
    notify();
    REFRESH
}

fn finish_failure(notify: fn()) {
    if let Ok(mut state) = STATE.lock() {
        state.snapshot.phase = Phase::Failed;
    }
    notify();
}

fn should_notify(current: [u32; 3], release: &Release, notified: Option<String>) -> bool {
    release.number > current && notified.as_deref() != Some(release.version.as_str())
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
}

fn fetch() -> Result<Release> {
    let body = http::get("api.github.com", ENDPOINT, "Accept: application/vnd.github+json\r\n", MAX_BODY)?;
    parse_release(&body)
}

fn parse_release(body: &str) -> Result<Release> {
    let release: ApiRelease = serde_json::from_str(body).map_err(|e| invalid(e.to_string()))?;
    let (version, number) = parse_version(&release.tag_name)
        .map(|number| (format!("{}.{}.{}", number[0], number[1], number[2]), number))
        .ok_or_else(|| invalid(format!("unsupported release tag: {}", release.tag_name)))?;
    if !release.html_url.starts_with("https://github.com/lyoraeth/polynya/releases/") {
        return Err(invalid("unexpected release URL".into()));
    }
    Ok(Release { version, url: release.html_url, number })
}

fn parse_version(text: &str) -> Option<[u32; 3]> {
    let core = text.trim().trim_start_matches(['v', 'V']).split(['+', '-']).next()?;
    let mut parts = core.split('.');
    let mut version = [0; 3];
    for part in &mut version {
        *part = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(version)
}

#[derive(Serialize, Deserialize)]
struct Stored {
    notified_version: Option<String>,
}

fn load_notified() -> Option<String> {
    let path = proto::data_dir().join("update.json");
    let body = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Stored>(&body) {
        Ok(stored) => stored.notified_version,
        Err(e) => {
            proto::warn!("updates", "could not read update state: {e}");
            None
        }
    }
}

fn save_notified(version: &str) {
    let path = proto::data_dir().join("update.json");
    let result: std::result::Result<(), Box<dyn std::error::Error>> = (|| {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string(&Stored { notified_version: Some(version.into()) })?;
        std::fs::write(path, body)?;
        Ok(())
    })();
    if let Err(e) = result {
        proto::warn!("updates", "could not save update state: {e}");
    }
}

fn invalid(message: String) -> Error {
    Error::new(windows::core::HRESULT(-1), message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_numeric_and_ignore_build_suffixes() {
        assert_eq!(parse_version("v1.10.0"), Some([1, 10, 0]));
        assert_eq!(parse_version("1.10.0+5.gabcdef"), Some([1, 10, 0]));
        assert_eq!(parse_version("1.0.0-dev.12+abcdef"), Some([1, 0, 0]));
        assert!(parse_version("1.10").is_none());
        assert!(parse_version("v1.10.x").is_none());
        assert!([1, 10, 0] > [1, 9, 3]);
    }

    #[test]
    fn parses_latest_release() {
        let release = parse_release(r#"{"tag_name":"v1.2.0","html_url":"https://github.com/lyoraeth/polynya/releases/tag/v1.2.0","draft":false}"#).unwrap();
        assert_eq!(release.version, "1.2.0");
        assert_eq!(release.number, [1, 2, 0]);
    }

    #[test]
    fn notification_is_once_per_version() {
        let release = parse_release(r#"{"tag_name":"v1.2.0","html_url":"https://github.com/lyoraeth/polynya/releases/tag/v1.2.0"}"#).unwrap();
        assert!(should_notify([1, 1, 1], &release, None));
        assert!(!should_notify([1, 1, 1], &release, Some("1.2.0".into())));
        assert!(!should_notify([1, 2, 0], &release, None));
    }

    /// Live GitHub API: `cargo test --release latest_release_live -- --ignored`.
    #[test]
    #[ignore]
    fn latest_release_live() {
        let release = fetch().unwrap();
        assert!(!release.version.is_empty());
    }
}
