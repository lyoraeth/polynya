// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio levels for the visualizer: capture of the player's sound and a per-band spectrum.
//!
//! Only the app whose media session is active is captured (with its child processes: browsers play
//! sound in a separate process), so notifications and games do not move the bars. If the player's
//! process cannot be found, all sound of the default output device is captured.
//!
//! Returns raw levels without smoothing: drawing smooths by actual frame time. Analysis and frames
//! run at different rates, and smoothing on the analysis side caused jerks.

use std::{
    f32::consts::PI,
    mem::ManuallyDrop,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
            AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
            ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
            IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, eConsole, eRender,
        },
        System::{
            Com::{
                BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IAgileObject, IAgileObject_Impl,
                StructuredStorage::PROPVARIANT,
            },
            Threading::{CreateEventW, SetEvent, WaitForSingleObject},
            Variant::VT_BLOB,
        },
    },
    core::{HRESULT, IUnknown, Interface, Ref, Result, implement},
};

/// Number of bands: the visualizer bars.
pub const BANDS: usize = 5;
/// Band edges, Hz: from bass to upper mids; above that music has little energy.
const EDGES: [f32; BANDS + 1] = [60.0, 150.0, 400.0, 1000.0, 2500.0, 8000.0];
/// FFT window size: at 48 kHz about 21 ms of sound, ~47 Hz resolution. A smaller window smears
/// the bass: a kick drum (50–100 Hz) would land in one or two coarse bins.
const N: usize = 1024;
/// Compensation for music's natural energy roll-off toward high frequencies, dB per octave.
/// Partial: without it the upper bands would always read as weak.
const TILT_DB_PER_OCT: f32 = 2.0;
/// Time constant of a band's background, i.e. its average loudness, s.
const BASELINE_TAU: f32 = 1.5;
/// Bar height is how much louder a band is than its background: at background level the bar
/// stands at `REST_DB / (REST_DB + PUNCH_DB)` of the height (a small wave), at `PUNCH_DB` above
/// it at full height (a kick drum).
const REST_DB: f32 = 4.0;
const PUNCH_DB: f32 = 12.0;
/// A band whose background is this much quieter than the loudest one is drawn at minimum scale:
/// a quiet synth in the mids stays small next to a pronounced bass.
const WEAK_DB: f32 = 20.0;
const WEAK_SCALE: f32 = 0.35;
/// Below this level a band counts as silence.
const SILENCE_DB: f32 = -70.0;
/// Analysis rate: 20 times per second. Drawing runs more often and eases the bars toward the
/// levels itself, so an exact spectrum per frame is not needed.
const POLL: Duration = Duration::from_millis(50);
/// Less often in silence: bars come alive with up to this delay, unnoticeable by eye. The capture
/// buffer is 200 ms, so polling must drain it in time.
const POLL_SILENT: Duration = Duration::from_millis(150);
/// How long without sound counts as silence.
const SILENT_AFTER: Duration = Duration::from_secs(1);

static LEVELS: Mutex<[f32; BANDS]> = Mutex::new([0.0; BANDS]);
/// The island is hidden (fullscreen app): no analysis needed.
static PAUSED: AtomicBool = AtomicBool::new(false);
/// Polling while paused: the capture buffer is 200 ms, so drain it before it overflows.
const POLL_PAUSED: Duration = Duration::from_millis(100);

pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Relaxed);
}

/// Current band levels, 0..1.
pub fn levels() -> [f32; BANDS] {
    LEVELS.lock().map(|l| *l).unwrap_or_default()
}

// ── Capture Target ──

/// App of the active media session (AppUserModelId) and its change counter.
static TARGET: Mutex<Option<String>> = Mutex::new(None);
static TARGET_GEN: AtomicU64 = AtomicU64::new(0);

/// Sets whose sound to capture. Called when the media session changes.
pub fn set_target(app: Option<String>) {
    let Ok(mut t) = TARGET.lock() else { return };
    if *t != app {
        *t = app;
        TARGET_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

// ── Capture ──

pub fn start() {
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(|| {
            // SAFETY: initializes COM for this new thread before any audio call on it.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            // Capture restarts on a player change (at once) and on an error, when the output device
            // changed or disappeared (after a second).
            loop {
                let failed = capture().map_err(|e| proto::warn!("audio", "capture: {e}")).is_err();
                if let Ok(mut l) = LEVELS.lock() {
                    *l = [0.0; BANDS];
                }
                if failed {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        })
        .ok();
}

/// Format requested from process capture: it has no mix format of its own.
const PROCESS_FORMAT: WAVEFORMATEX = WAVEFORMATEX {
    wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
    nChannels: 2,
    nSamplesPerSec: 48_000,
    nAvgBytesPerSec: 48_000 * 8,
    nBlockAlign: 8,
    wBitsPerSample: 32,
    cbSize: 0,
};
/// Capture buffer of 200 ms, with headroom over polling.
const BUFFER_HNS: i64 = 2_000_000;

/// Reports completion of the async activation by setting the event the thread waits on.
#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(isize);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _op: Ref<IActivateAudioInterfaceAsyncOperation>) -> Result<()> {
        // SAFETY: `self.0` is the event handle created in `process_client`, which keeps it open until
        // the wait ends or times out; setting a closed handle only fails.
        unsafe { SetEvent(HANDLE(self.0 as _)) }
    }
}

impl IAgileObject_Impl for Activated_Impl {}

/// Capture client for the sound of process `pid` together with its child processes.
fn process_client(pid: u32) -> Result<IAudioClient> {
    // SAFETY: `params` and the PROPVARIANT pointing to it live on this frame for the whole activation
    // call; `event` is closed on every path; all other COM calls use live objects.
    unsafe {
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // The PROPVARIANT is not freed: its blob points to `params` on the stack, not to the COM heap.
        let mut pv = ManuallyDrop::new(PROPVARIANT::default());
        let inner = &mut pv.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB { cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: &params as *const _ as *mut u8 };

        let event = CreateEventW(None, false, false, None)?;
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(event.0 as isize).into();
        let op = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*pv), &handler);
        let op = match op {
            Ok(op) => op,
            Err(e) => {
                let _ = CloseHandle(event);
                return Err(e);
            }
        };
        WaitForSingleObject(event, 5_000);
        let _ = CloseHandle(event);
        let (mut hr, mut unk): (HRESULT, Option<IUnknown>) = (HRESULT(0), None);
        op.GetActivateResult(&mut hr, &mut unk)?;
        hr.ok()?;
        let client: IAudioClient = unk.ok_or_else(|| windows::core::Error::from(HRESULT(-1)))?.cast()?;
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, BUFFER_HNS, 0, &PROCESS_FORMAT, None)?;
        Ok(client)
    }
}

/// Capture client for all sound of the default output device, and its format.
fn system_client() -> Result<(IAudioClient, WAVEFORMATEX)> {
    // SAFETY: `fmt_ptr` is a valid WAVEFORMATEX from `GetMixFormat`, copied and passed to `Initialize`
    // before it is freed exactly once.
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let fmt_ptr = client.GetMixFormat()?;
        let fmt: WAVEFORMATEX = *fmt_ptr;
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, BUFFER_HNS, 0, fmt_ptr, None);
        CoTaskMemFree(Some(fmt_ptr as _));
        init?;
        Ok((client, fmt))
    }
}

/// Opens capture for the current target: the player's sound, or all sound if its process is not found.
fn open() -> Result<(IAudioClient, WAVEFORMATEX)> {
    let app = TARGET.lock().ok().and_then(|t| t.clone());
    if let Some(app) = &app {
        match crate::apps::find_process(app) {
            Some((exe, pid)) => match process_client(pid) {
                Ok(client) => {
                    proto::info!("audio", "capturing {exe} (pid {pid})");
                    return Ok((client, PROCESS_FORMAT));
                }
                Err(e) => proto::warn!("audio", "capture of {exe} failed, using system mix: {e}"),
            },
            None => proto::warn!("audio", "no process for {app}, using system mix"),
        }
    }
    system_client()
}

/// Captures and analyzes until the player changes (returns `Ok`) or the device fails.
fn capture() -> Result<()> {
    let generation = TARGET_GEN.load(Ordering::Relaxed);
    let (client, fmt) = open()?;
    // SAFETY: each `GetBuffer` yields `frames` frames of `channels` samples in the device format,
    // read only until the matching `ReleaseBuffer`; a non-float format is at least 16-bit, so i16
    // reads stay inside the packet.
    unsafe {
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        let channels = fmt.nChannels.max(1) as usize;
        // The Windows shared mixer almost always gives float32; 16-bit PCM is a fallback.
        let float = fmt.wBitsPerSample == 32;
        let rate = fmt.nSamplesPerSec as f32;
        let mut ring = vec![0f32; N];
        let mut pos = 0usize;
        let mut analyzer = Analyzer::new(rate);
        let mut last_audio = Instant::now();
        let mut last_analysis = Instant::now();

        loop {
            // The player changed: reopen capture on the new process.
            if TARGET_GEN.load(Ordering::Relaxed) != generation {
                let _ = client.Stop();
                return Ok(());
            }
            let paused = PAUSED.load(Ordering::Relaxed);
            let silent = last_audio.elapsed() > SILENT_AFTER;
            std::thread::sleep(if paused {
                POLL_PAUSED
            } else if silent {
                POLL_SILENT
            } else {
                POLL
            });
            if paused {
                // Packets are drained without analysis so stale sound is not read after the pause.
                while cap.GetNextPacketSize()? > 0 {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                    cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    cap.ReleaseBuffer(frames)?;
                }
                continue;
            }
            let mut fresh = false;
            while cap.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                for f in 0..frames as usize {
                    let mut sum = 0.0;
                    if !silent {
                        for c in 0..channels {
                            let i = f * channels + c;
                            sum += if float { *(data as *const f32).add(i) } else { *(data as *const i16).add(i) as f32 / 32768.0 };
                        }
                    }
                    ring[pos] = sum / channels as f32;
                    pos = (pos + 1) % N;
                }
                cap.ReleaseBuffer(frames)?;
                fresh |= frames > 0 && !silent;
            }
            if fresh {
                last_audio = Instant::now();
            }
            // Without sound no packets arrive at all, and levels simply fade.
            let dt = last_analysis.elapsed().as_secs_f32();
            last_analysis = Instant::now();
            let bands = if last_audio.elapsed() < Duration::from_millis(100) { analyzer.bands(&ring, pos, dt) } else { [0.0; BANDS] };
            if let Ok(mut l) = LEVELS.lock() {
                *l = bands;
            }
        }
    }
}

struct Analyzer {
    window: Vec<f32>,
    /// FFT bin ranges for each band.
    bins: [(usize, usize); BANDS],
    /// Spectrum tilt correction for each band, dB.
    tilt: [f32; BANDS],
    /// Each band's background, its average loudness over the last seconds, dB; `None` before the first sound.
    baseline: Option<[f32; BANDS]>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Analyzer {
    fn new(rate: f32) -> Self {
        // A Hann window reduces energy leaking into neighboring bins.
        let window = (0..N).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (N - 1) as f32).cos()).collect();
        let bin = |hz: f32| ((hz / rate * N as f32).round() as usize).clamp(1, N / 2 - 1);
        let mut bins = [(0, 0); BANDS];
        let mut tilt = [0.0; BANDS];
        let center = |i: usize| (EDGES[i] * EDGES[i + 1]).sqrt();
        for i in 0..BANDS {
            bins[i] = (bin(EDGES[i]), bin(EDGES[i + 1]).max(bin(EDGES[i]) + 1));
            tilt[i] = TILT_DB_PER_OCT * (center(i) / center(0)).log2();
        }
        Self { window, bins, tilt, baseline: None, re: vec![0.0; N], im: vec![0.0; N] }
    }

    /// Band levels 0..1 from the last `N` samples of the ring buffer starting at `pos`; `dt` is the
    /// time since the previous analysis, s.
    ///
    /// A bar shows the band's dynamics, not its absolute loudness: in music the bands are nearly equal
    /// after tilt compensation, and an absolute scale would put all bars at one height. A kick drum is a
    /// jump above the background, a steady synth is the background.
    fn bands(&mut self, ring: &[f32], pos: usize, dt: f32) -> [f32; BANDS] {
        for i in 0..N {
            self.re[i] = ring[(pos + i) % N] * self.window[i];
            self.im[i] = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        // Normalized by the window sum: a sine of amplitude 1.0 gives about 0 dB.
        let norm = (N as f32 / 4.0).powi(2);
        let mut db = [0.0; BANDS];
        for (i, &(a, b)) in self.bins.iter().enumerate() {
            // Average power per bin, not the sum: otherwise wide upper bands would gain loudness from their
            // width alone.
            let mean = (a..b).map(|k| self.re[k] * self.re[k] + self.im[k] * self.im[k]).sum::<f32>() / (b - a) as f32;
            db[i] = 10.0 * (mean / norm).max(1e-12).log10() + self.tilt[i];
        }
        let k = 1.0 - (-dt / BASELINE_TAU).exp();
        let base = self.baseline.get_or_insert(db);
        for (b, d) in base.iter_mut().zip(db) {
            *b += (d - *b) * k;
        }
        let loudest = base.iter().copied().fold(f32::MIN, f32::max);
        let mut out = [0.0; BANDS];
        for i in 0..BANDS {
            if db[i] < SILENCE_DB {
                continue;
            }
            let punch = ((db[i] - base[i] + REST_DB) / (REST_DB + PUNCH_DB)).clamp(0.0, 1.0);
            let strength = ((base[i] - (loudest - WEAK_DB)) / WEAK_DB).clamp(0.0, 1.0);
            out[i] = punch * (WEAK_SCALE + (1.0 - WEAK_SCALE) * strength);
        }
        out
    }
}

/// Iterative in-place radix-2 FFT. The length is a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_sine_bin() {
        let k = 37;
        let mut re: Vec<f32> = (0..N).map(|i| (2.0 * PI * k as f32 * i as f32 / N as f32).sin()).collect();
        let mut im = vec![0.0; N];
        fft(&mut re, &mut im);
        let mag = |i: usize| (re[i] * re[i] + im[i] * im[i]).sqrt();
        let peak = (1..N / 2).max_by(|&a, &b| mag(a).total_cmp(&mag(b))).unwrap();
        assert_eq!(peak, k);
    }

    const RATE: f32 = 48_000.0;

    fn tone(hz: f32, amp: f32, i: usize) -> f32 {
        amp * (2.0 * PI * hz * i as f32 / RATE).sin()
    }

    /// Runs `frames` analysis frames of the same signal and returns the last one.
    fn settle(a: &mut Analyzer, signal: &[f32], frames: usize) -> [f32; BANDS] {
        (0..frames).map(|_| a.bands(signal, 0, 0.02)).last().unwrap()
    }

    /// A steady tone: a small wave in its band, the other bands silent.
    #[test]
    fn steady_tone_is_a_small_wave() {
        let mut a = Analyzer::new(RATE);
        // 700 Hz is the middle of the third band (400–1000).
        let ring: Vec<f32> = (0..N).map(|i| tone(700.0, 0.5, i)).collect();
        let b = settle(&mut a, &ring, 100);
        assert!(b[2] > 0.15 && b[2] < 0.4, "{b:?}");
        assert!(b[0] < 0.1 && b[4] < 0.1, "{b:?}");
    }

    /// A kick drum over a steady background: the bass jumps almost to the top, a quiet synth stays small.
    #[test]
    fn kick_punches_quiet_synth_stays_small() {
        let mut a = Analyzer::new(RATE);
        // Between hits: quiet bass and a mid synth 15 dB below the bass hit.
        let calm: Vec<f32> = (0..N).map(|i| tone(70.0, 0.1, i) + tone(800.0, 0.5 * 0.178, i)).collect();
        settle(&mut a, &calm, 100);
        let kick: Vec<f32> = (0..N).map(|i| tone(70.0, 0.5, i) + tone(800.0, 0.5 * 0.178, i)).collect();
        let b = a.bands(&kick, 0, 0.02);
        assert!(b[0] > 0.8, "{b:?}");
        assert!(b[2] < 0.4, "{b:?}");
    }

    /// Silence: the bars are at zero.
    #[test]
    fn silence_is_zero() {
        let mut a = Analyzer::new(RATE);
        assert_eq!(a.bands(&vec![0.0; N], 0, 0.02), [0.0; BANDS]);
    }
}
