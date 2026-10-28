// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows media sessions (GSMTC): what is playing, playback control, cover art.
//!
//! Runs in its own MTA thread. GSMTC events arrive from the thread pool, and the UI thread must not
//! wait on WinRT operations: handlers update shared state and call `notify`, while UI commands go
//! through a channel and run here.
//!
//! The session is chosen here instead of taking Windows' "current" one: that is the last one
//! interacted with, so a paused browser tab overrides playing Spotify. Rule: a playing session (of
//! several, the Windows current one); if nothing plays, stay on the previous one while it lives.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use windows::{
    Foundation::TypedEventHandler,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionMediaProperties as Properties, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    },
    Storage::Streams::DataReader,
    Win32::System::{
        Com::{COINIT_MULTITHREADED, CoInitializeEx},
        SystemInformation::GetSystemTimeAsFileTime,
    },
    core::Result,
};

/// Ticks (100 ns) per second: the unit of WinRT TimeSpan and DateTime.
const TICKS: f64 = 10_000_000.0;
/// Cover rereads after a track change: players (Spotify especially) report the new title before
/// swapping the picture, and a second event may never come.
const COVER_RETRIES: [u64; 3] = [400, 1200, 2500];
/// Property rereads after an event about them: Spotify sends the event before updating the
/// properties, so an immediate read can still return the previous track.
const PROPS_RECHECK: [u64; 2] = [500, 1500];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone)]
pub struct State {
    pub track: Option<Track>,
    pub playing: bool,
    /// Position in seconds as of `sampled`.
    position: f64,
    sampled: Instant,
    pub duration: f64,
    /// Cover in its original format (usually JPEG/PNG).
    pub cover: Option<Arc<Vec<u8>>>,
    /// Cover change counter, so the UI decodes it only when it changes.
    pub cover_rev: u64,
}

impl Default for State {
    fn default() -> Self {
        Self { track: None, playing: false, position: 0.0, sampled: Instant::now(), duration: 0.0, cover: None, cover_rev: 0 }
    }
}

impl State {
    /// Current position, accounting for the time since the last update.
    ///
    /// Players update the position rarely (Spotify every few seconds and on events), so during
    /// playback it is extrapolated.
    pub fn position(&self) -> f64 {
        let p = if self.playing { self.position + self.sampled.elapsed().as_secs_f64() } else { self.position };
        p.clamp(0.0, self.duration.max(0.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    TogglePlay,
    Next,
    Prev,
    /// Seeks to a position in seconds.
    Seek(f64),
}

pub struct Media {
    pub state: Arc<Mutex<State>>,
    tx: mpsc::Sender<Cmd>,
}

impl Media {
    /// Starts the media session thread. `notify` is called from any thread on every change.
    pub fn start(notify: fn()) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        std::thread::Builder::new()
            .name("media".into())
            .spawn(move || {
                if let Err(e) = run(shared, notify, rx) {
                    proto::error!("media", "{e}");
                }
            })
            .ok();
        Self { state, tx }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// The chosen session and its subscription tokens, to unsubscribe on switch.
struct Current {
    session: Session,
    app: String,
    tokens: [i64; 3],
}

/// Cover: the current one and the previous track's; for a while the player serves the old picture
/// with the new title.
#[derive(Default)]
struct Cover {
    hash: u64,
    prev: u64,
    /// The track changed, but the new picture has not arrived yet.
    pending: bool,
}

struct Shared {
    state: Arc<Mutex<State>>,
    notify: fn(),
    manager: Manager,
    current: Mutex<Option<Current>>,
    /// Status subscriptions of all sessions: when another starts playing, switch to it.
    watched: Mutex<Vec<(Session, i64)>>,
    cover: Mutex<Cover>,
}

fn run(state: Arc<Mutex<State>>, notify: fn(), rx: mpsc::Receiver<Cmd>) -> Result<()> {
    // SAFETY: initializes COM for this new thread before any WinRT call on it.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let manager = Manager::RequestAsync()?.join()?;
    let shared = Arc::new(Shared {
        state,
        notify,
        manager: manager.clone(),
        current: Mutex::new(None),
        watched: Mutex::new(Vec::new()),
        cover: Mutex::new(Cover::default()),
    });

    watch_all(&shared);
    choose(&shared);
    let s = shared.clone();
    manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
        choose(&s);
        Ok(())
    }))?;
    let s = shared.clone();
    manager.SessionsChanged(&TypedEventHandler::new(move |_, _| {
        watch_all(&s);
        choose(&s);
        Ok(())
    }))?;

    // The thread lives while the command sender does, i.e. for the app's whole lifetime.
    while let Ok(cmd) = rx.recv() {
        let session = shared.current.lock().ok().and_then(|c| c.as_ref().map(|c| c.session.clone()));
        let Some(session) = session else { continue };
        let _ = match cmd {
            Cmd::TogglePlay => session.TryTogglePlayPauseAsync().and_then(|op| op.join()),
            Cmd::Next => session.TrySkipNextAsync().and_then(|op| op.join()),
            Cmd::Prev => session.TrySkipPreviousAsync().and_then(|op| op.join()),
            Cmd::Seek(secs) => session.TryChangePlaybackPositionAsync((secs * TICKS) as i64).and_then(|op| op.join()),
        };
    }
    Ok(())
}

fn app_id(s: &Session) -> String {
    s.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default()
}

fn is_playing(s: &Session) -> bool {
    s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).map(|s| s == Status::Playing).unwrap_or(false)
}

/// Resubscribes to the status of all sessions: the session list changed.
fn watch_all(shared: &Arc<Shared>) {
    let Ok(mut watched) = shared.watched.lock() else { return };
    for (s, t) in watched.drain(..) {
        let _ = s.RemovePlaybackInfoChanged(t);
    }
    let Ok(sessions) = shared.manager.GetSessions() else { return };
    for s in sessions {
        let sh = shared.clone();
        if let Ok(t) = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            choose(&sh);
            Ok(())
        })) {
            watched.push((s, t));
        }
    }
}

/// Picks a session by the rule in the module header and switches if it is a different one.
fn choose(shared: &Arc<Shared>) {
    let sessions: Vec<Session> = shared.manager.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default();
    let system = shared.manager.GetCurrentSession().ok();
    let system_app = system.as_ref().map(app_id);
    let held = shared.current.lock().ok().and_then(|c| c.as_ref().map(|c| c.app.clone()));
    let playing: Vec<&Session> = sessions.iter().filter(|s| is_playing(s)).collect();
    let pick = if !playing.is_empty() {
        // Of several playing, the Windows current one, otherwise the one we are on.
        playing
            .iter()
            .find(|s| Some(app_id(s)) == system_app)
            .or_else(|| playing.iter().find(|s| Some(app_id(s)) == held))
            .or(playing.first())
            .map(|s| (*s).clone())
    } else {
        sessions.iter().find(|s| Some(app_id(s)) == held).cloned().or(system)
    };
    let pick_app = pick.as_ref().map(app_id);
    if pick_app == held && pick.is_some() {
        return;
    }
    attach(shared, pick);
}

/// Switches to a session: unsubscribes from the previous, subscribes to the new one.
fn attach(shared: &Arc<Shared>, session: Option<Session>) {
    let Ok(mut current) = shared.current.lock() else { return };
    if let Some(Current { session: old, tokens: [a, b, c], .. }) = current.take() {
        let _ = old.RemoveMediaPropertiesChanged(a);
        let _ = old.RemovePlaybackInfoChanged(b);
        let _ = old.RemoveTimelinePropertiesChanged(c);
    }

    let Some(session) = session else {
        drop(current);
        crate::audio::set_target(None);
        if let Ok(mut st) = shared.state.lock() {
            *st = State { cover_rev: st.cover_rev + 1, ..State::default() };
        }
        if let Ok(mut c) = shared.cover.lock() {
            *c = Cover::default();
        }
        (shared.notify)();
        return;
    };

    let app = app_id(&session);
    let (s1, s2, s3) = (shared.clone(), shared.clone(), shared.clone());
    let tokens = [
        session.MediaPropertiesChanged(&TypedEventHandler::new(move |s: windows::core::Ref<Session>, _| {
            if let Ok(s) = s.ok() {
                refresh_properties(&s1, s, "properties");
                recheck_properties(&s1, s);
            }
            Ok(())
        })),
        // Properties are also reread on playback events: Spotify's properties event runs ahead of the
        // properties, and a second one may not come.
        session.PlaybackInfoChanged(&TypedEventHandler::new(move |s: windows::core::Ref<Session>, _| {
            // A status change also changes position extrapolation; reread both.
            if let Ok(s) = s.ok() {
                refresh_playback(&s2, s);
                refresh_properties(&s2, s, "playback");
            }
            Ok(())
        })),
        session.TimelinePropertiesChanged(&TypedEventHandler::new(move |s: windows::core::Ref<Session>, _| {
            if let Ok(s) = s.ok() {
                refresh_playback(&s3, s);
                refresh_properties(&s3, s, "timeline");
            }
            Ok(())
        })),
    ];
    let tokens = tokens.map(|t| t.unwrap_or_default());
    *current = Some(Current { session: session.clone(), app: app.clone(), tokens });
    drop(current);

    // The visualizer listens to this app only.
    proto::info!("media", "session: {}", if app.is_empty() { "-" } else { &app });
    crate::audio::set_target(Some(app).filter(|a| !a.is_empty()));

    refresh_properties(shared, &session, "session");
    refresh_playback(shared, &session);
}

/// Follow-up property rereads after an event about them.
fn recheck_properties(shared: &Arc<Shared>, session: &Session) {
    let (shared, session) = (shared.clone(), session.clone());
    let _ = std::thread::Builder::new().name("props".into()).spawn(move || {
        let mut waited = 0;
        for at in PROPS_RECHECK {
            std::thread::sleep(Duration::from_millis(at - waited));
            waited = at;
            refresh_properties(&shared, &session, "recheck");
        }
    });
}

/// The event came from the chosen session, not the previous one: handlers run on the thread pool,
/// and a late event from the old session would overwrite the new one.
fn is_current(shared: &Shared, s: &Session) -> bool {
    let app = app_id(s);
    shared.current.lock().ok().and_then(|c| c.as_ref().map(|c| c.app == app)).unwrap_or(false)
}

/// Time for the debug log.
fn refresh_properties(shared: &Arc<Shared>, session: &Session, why: &str) {
    if !is_current(shared, session) {
        return;
    }
    let Ok(props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.join()) else { return };
    let track = Track {
        title: props.Title().map(|s| s.to_string()).unwrap_or_default(),
        artist: props.Artist().map(|s| s.to_string()).unwrap_or_default(),
        album: props.AlbumTitle().map(|s| s.to_string()).unwrap_or_default(),
    };
    // An empty title: the player is open but nothing is selected. For a playing one it is the instant
    // between tracks (Spotify), and "nothing playing" would blink on it.
    if track.title.is_empty() && is_playing(session) {
        return;
    }
    let track = (!track.title.is_empty()).then_some(track);

    let changed = shared.state.lock().map(|st| st.track != track).unwrap_or(true);
    if changed {
        proto::debug!("media", "track ({why}): {}", track.as_ref().map_or("-".into(), |t| format!("{} - {}", t.artist, t.title)));
        if let Ok(mut st) = shared.state.lock() {
            st.track = track.clone();
        }
        if let Ok(mut c) = shared.cover.lock() {
            c.prev = c.hash;
            c.pending = track.is_some();
        }
        if track.is_some() {
            retry_cover(shared, session, track.clone());
        } else {
            set_cover(shared, None);
        }
    }
    // The cover may arrive after the title (browsers: YouTube) as a separate properties event for the
    // same track; the same picture is filtered out by hash.
    let (pending, none) = shared.cover.lock().map(|c| (c.pending, c.hash == 0)).unwrap_or((false, false));
    if pending || why == "properties" || (none && track.is_some()) {
        read_cover(shared, &props);
    }
    (shared.notify)();
}

/// Reads the cover and accepts it if it is already the new track's picture.
fn read_cover(shared: &Shared, props: &Properties) {
    let Some(bytes) = read_thumbnail(props) else { return };
    let hash = fnv(&bytes);
    let Ok(mut c) = shared.cover.lock() else { return };
    // The same picture as the previous track's: the player has not swapped it yet; wait for the next.
    if hash == c.prev {
        return;
    }
    c.pending = false;
    if hash == c.hash {
        return;
    }
    c.hash = hash;
    drop(c);
    set_cover(shared, Some(bytes));
}

fn set_cover(shared: &Shared, bytes: Option<Vec<u8>>) {
    if let Ok(mut st) = shared.state.lock() {
        st.cover = bytes.map(Arc::new);
        st.cover_rev += 1;
    }
}

/// Rereads the cover with pauses until it arrives, the track changes, or the session does.
fn retry_cover(shared: &Arc<Shared>, session: &Session, track: Option<Track>) {
    let (shared, session) = (shared.clone(), session.clone());
    let _ = std::thread::Builder::new().name("cover".into()).spawn(move || {
        let mut waited = 0;
        for at in COVER_RETRIES {
            std::thread::sleep(Duration::from_millis(at - waited));
            waited = at;
            let same = shared.state.lock().map(|st| st.track == track).unwrap_or(false);
            let pending = shared.cover.lock().map(|c| c.pending).unwrap_or(false);
            if !same || !pending || !is_current(&shared, &session) {
                return;
            }
            if let Ok(props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.join()) {
                read_cover(&shared, &props);
                if !shared.cover.lock().map(|c| c.pending).unwrap_or(false) {
                    (shared.notify)();
                    return;
                }
            }
        }
        // It never changed: the track has the same cover (same album), so keep it; or there is none at
        // all, so drop the previous one.
        if let Ok(mut c) = shared.cover.lock() {
            c.pending = false;
        }
        let none = session.TryGetMediaPropertiesAsync().and_then(|op| op.join()).map(|p| read_thumbnail(&p).is_none()).unwrap_or(false);
        if none && is_current(&shared, &session) {
            if let Ok(mut c) = shared.cover.lock() {
                c.hash = 0;
            }
            set_cover(&shared, None);
            (shared.notify)();
        }
    });
}

/// FNV-1a: tells a picture from the previous one.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn read_thumbnail(props: &Properties) -> Option<Vec<u8>> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(buf)
}

fn refresh_playback(shared: &Shared, session: &Session) {
    if !is_current(shared, session) {
        return;
    }
    let playing = is_playing(session);
    let Ok(tl) = session.GetTimelineProperties() else { return };
    let position = tl.Position().map(|t| t.Duration as f64 / TICKS).unwrap_or(0.0);
    let start = tl.StartTime().map(|t| t.Duration as f64 / TICKS).unwrap_or(0.0);
    let end = tl.EndTime().map(|t| t.Duration as f64 / TICKS).unwrap_or(0.0);
    // The player sampled the position at LastUpdatedTime, not now: add the time that has passed.
    let lag = tl
        .LastUpdatedTime()
        .ok()
        .map(|t| {
            // SAFETY: `GetSystemTimeAsFileTime` has no preconditions.
            let now = unsafe { GetSystemTimeAsFileTime() };
            let now = ((now.dwHighDateTime as i64) << 32 | now.dwLowDateTime as i64) as f64;
            ((now - t.UniversalTime as f64) / TICKS).max(0.0)
        })
        .filter(|_| playing)
        .unwrap_or(0.0);

    if let Ok(mut st) = shared.state.lock() {
        st.playing = playing;
        st.position = position - start + lag;
        st.sampled = Instant::now();
        st.duration = end - start;
    }
    (shared.notify)();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live system: what each session returns as its cover. `cargo test -- --ignored
    /// thumbnails_live --nocapture`
    #[test]
    #[ignore]
    fn thumbnails_live() {
        // SAFETY: initializes COM for the test thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let manager = Manager::RequestAsync().unwrap().join().unwrap();
        for s in manager.GetSessions().unwrap() {
            let props = s.TryGetMediaPropertiesAsync().unwrap().join().unwrap();
            let thumb = props.Thumbnail();
            let size = thumb.as_ref().ok().and_then(|t| t.OpenReadAsync().ok()).and_then(|op| op.join().ok()).map(|st| st.Size().unwrap_or(0));
            println!(
                "{} | {} — {} | thumbnail: {} | size: {:?}",
                app_id(&s),
                props.Artist().unwrap_or_default(),
                props.Title().unwrap_or_default(),
                thumb.is_ok(),
                size
            );
        }
    }
}
