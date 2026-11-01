//! System media now-playing + transport (SPEC C5).
//!
//! Reads the Windows `GlobalSystemMediaTransportControlsSessionManager` (GSMTC)
//! current session — the same track metadata the OS shows on the media flyout —
//! and can issue transport commands (play/pause/next/prev). It is strictly
//! **read-only with respect to content**: it captures **no audio**, only the
//! title/artist/album/position/artwork the system already publishes.
//!
//! ## Threading (this is what v0.1 got wrong)
//! WinRT async operations block with `.get()`. Tauri runs *synchronous* commands on
//! the main (STA) thread, where that blocking call deadlocks or stalls the UI. So
//! **all** GSMTC work happens on one dedicated background thread
//! ([`MediaWorker`]) that first calls `RoInitialize(RO_INIT_MULTITHREADED)`.
//! It polls the session every ~500 ms, runs transport commands requested from other
//! threads (over a channel, so the call is always made on the MTA thread), and
//! hands every *meaningful* change to a callback; deciding what is meaningful is
//! the pure [`MediaTracker`].
//!
//! Everything that does not touch the OS — the wire DTOs, position extrapolation,
//! change detection, artwork shrinking, source-name cleanup — is pure and tested.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde::Serialize;

use crate::imaging;
use crate::SysError;

/// How often the worker re-reads the session.
pub const POLL: Duration = Duration::from_millis(500);
/// While playing, an unchanged state is still re-emitted this often (ms) so the UI
/// can re-anchor its interpolated position.
pub const RESYNC_MS: u64 = 5_000;
/// Artwork bigger than this (bytes) is re-encoded smaller if it is a PNG, else dropped.
pub const ARTWORK_CAP_BYTES: usize = 200 * 1024;
/// Never even read an artwork stream larger than this.
pub const ARTWORK_READ_MAX_BYTES: usize = 3 * 1024 * 1024;

/// What is currently playing, as the system reports it (legacy whole-second DTO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_sec: u64,
    pub duration_sec: u64,
}

/// The `media-changed` payload (docs/CONTRACTS.md §2).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_sec: f64,
    pub duration_sec: f64,
    /// Epoch ms at which `position_sec` was sampled; the UI interpolates from here.
    pub sampled_at: u64,
    /// e.g. `Spotify.exe`, `chrome.exe`, `MSEdge`.
    pub source_app: String,
    /// `data:image/…;base64,…` thumbnail, if the app published one.
    pub artwork: Option<String>,
}

impl MediaState {
    /// The same state with the position advanced to `now_ms` (only while playing).
    pub fn advanced_to(&self, now_ms: u64) -> MediaState {
        let mut s = self.clone();
        if self.playing && now_ms > self.sampled_at {
            let mut pos = self.position_sec + (now_ms - self.sampled_at) as f64 / 1000.0;
            if self.duration_sec > 0.0 {
                pos = pos.min(self.duration_sec);
            }
            s.position_sec = pos;
        }
        s.sampled_at = now_ms.max(self.sampled_at);
        s
    }

    /// Whole-second legacy view.
    pub fn as_track(&self) -> MediaTrack {
        MediaTrack {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            playing: self.playing,
            position_sec: self.position_sec.max(0.0) as u64,
            duration_sec: self.duration_sec.max(0.0) as u64,
        }
    }
}

/// A transport action to send to the active media session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportCommand {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
}

impl TransportCommand {
    /// Canonical lowercase name (used on the wire from the frontend).
    pub fn as_str(self) -> &'static str {
        match self {
            TransportCommand::Play => "play",
            TransportCommand::Pause => "pause",
            TransportCommand::PlayPause => "playpause",
            TransportCommand::Next => "next",
            TransportCommand::Previous => "previous",
        }
    }

    /// Parse a wire command (case-insensitive). `prev` is accepted as an alias
    /// for `previous`. Returns `None` for anything unrecognized.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "play" => Some(TransportCommand::Play),
            "pause" => Some(TransportCommand::Pause),
            "playpause" | "toggle" => Some(TransportCommand::PlayPause),
            "next" | "skip" => Some(TransportCommand::Next),
            "previous" | "prev" => Some(TransportCommand::Previous),
            _ => None,
        }
    }
}

/// Playback progress in `[0.0, 1.0]`; `0.0` when the duration is unknown.
pub fn progress_fraction(position_sec: u64, duration_sec: u64) -> f64 {
    if duration_sec == 0 {
        0.0
    } else {
        (position_sec as f64 / duration_sec as f64).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// 100 ns ticks between the Windows epoch (1601-01-01) and the Unix epoch.
const WIN_EPOCH_OFFSET_TICKS: i64 = 116_444_736_000_000_000;

/// Unix epoch milliseconds → Windows `DateTime.UniversalTime` ticks.
pub fn unix_ms_to_win_ticks(ms: u64) -> i64 {
    (ms as i64)
        .saturating_mul(10_000)
        .saturating_add(WIN_EPOCH_OFFSET_TICKS)
}

/// `(position_sec, duration_sec)` for a GSMTC timeline.
///
/// GSMTC reports `position` as of `last_updated`, not as of now, so while playing
/// the position is extrapolated by the elapsed wall time × playback rate (this is
/// what the Windows media flyout does). Timeline values are 100 ns ticks;
/// `last_updated`/`now` are Windows-epoch ticks. A session with no duration
/// (live stream, or an app that publishes no timeline) is reported as-is, with no
/// extrapolation.
pub fn timeline_seconds(
    position: i64,
    start: i64,
    end: i64,
    last_updated: i64,
    now: i64,
    playing: bool,
    rate: f64,
) -> (f64, f64) {
    const TICKS_PER_SEC: f64 = 10_000_000.0;
    let duration = if end > start {
        (end - start) as f64 / TICKS_PER_SEC
    } else {
        0.0
    };
    let mut pos = ((position - start).max(0)) as f64 / TICKS_PER_SEC;
    if duration > 0.0 && playing && last_updated > 0 && now > last_updated {
        let rate = if rate.is_finite() {
            rate.clamp(0.0, 4.0)
        } else {
            1.0
        };
        pos += (now - last_updated) as f64 / TICKS_PER_SEC * rate;
    }
    if duration > 0.0 {
        pos = pos.min(duration);
    }
    (round1(pos), round1(duration))
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Clean an `SourceAppUserModelId`: classic apps look like `Spotify.exe` (kept);
/// packaged apps look like `Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic`
/// (reduced to the part after `!`).
pub fn friendly_source(aumid: &str) -> String {
    let t = aumid.trim();
    match t.split_once('!') {
        Some((pkg, app)) if pkg.contains('_') && !app.is_empty() => app.to_string(),
        _ => t.to_string(),
    }
}

/// Turn raw artwork bytes into a data URL no bigger than [`ARTWORK_CAP_BYTES`]:
/// small images pass through untouched; an oversize PNG is re-encoded smaller;
/// anything else oversize (or unrecognized) is dropped.
pub fn shrink_artwork(bytes: &[u8]) -> Option<String> {
    let mime = imaging::sniff_mime(bytes)?;
    if bytes.len() <= ARTWORK_CAP_BYTES {
        return Some(imaging::data_url(mime, bytes));
    }
    if mime != "image/png" {
        return None;
    }
    let (w, h, rgba) = imaging::decode_png(bytes)?;
    let thumb = imaging::make_thumbnail(w, h, &rgba, 320, ARTWORK_CAP_BYTES)?;
    Some(imaging::data_url("image/png", &thumb.png))
}

/// Poll cadence of artwork retries after the first [`ART_FAST_POLLS`] polls.
pub const ART_RETRY_EVERY: u32 = 10;
/// Polls (of the same track) during which a missing thumbnail is retried every time.
pub const ART_FAST_POLLS: u32 = 8;
/// Give up on a track's artwork after this many polls (~2 minutes at [`POLL`]).
pub const ART_GIVE_UP_POLLS: u32 = 240;

/// Whether poll number `polls` (0-based, per track) should try to read missing artwork.
pub fn art_attempt_due(polls: u32) -> bool {
    polls < ART_FAST_POLLS || (polls < ART_GIVE_UP_POLLS && polls % ART_RETRY_EVERY == 0)
}

/// Decides which samples are worth emitting as `media-changed`.
///
/// Emit when: it is the first sample; the track, source, play state, duration or
/// artwork changed; the session appeared/disappeared; the position jumped (a seek)
/// away from where extrapolation predicted; or while playing every [`RESYNC_MS`].
#[derive(Debug, Default)]
pub struct MediaTracker {
    started: bool,
    last: Option<MediaState>,
    last_emit_ms: u64,
}

impl MediaTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a sample; `Some(payload)` means "emit this" (the payload itself may be
    /// `None`, meaning the session went away).
    pub fn observe(
        &mut self,
        sample: Option<MediaState>,
        now_ms: u64,
    ) -> Option<Option<MediaState>> {
        let emit = if !self.started {
            true
        } else {
            match (&self.last, &sample) {
                (None, None) => false,
                (None, Some(_)) | (Some(_), None) => true,
                (Some(prev), Some(cur)) => {
                    Self::identity_changed(prev, cur)
                        || Self::sought(prev, cur)
                        || (cur.playing && now_ms.saturating_sub(self.last_emit_ms) >= RESYNC_MS)
                }
            }
        };
        self.started = true;
        // Always remember the freshest sample so seek detection compares against it.
        self.last = sample.clone();
        if emit {
            self.last_emit_ms = now_ms;
            Some(sample)
        } else {
            None
        }
    }

    fn identity_changed(a: &MediaState, b: &MediaState) -> bool {
        a.title != b.title
            || a.artist != b.artist
            || a.album != b.album
            || a.source_app != b.source_app
            || a.playing != b.playing
            || (a.duration_sec - b.duration_sec).abs() >= 1.0
            || a.artwork != b.artwork
    }

    /// A position that differs from the extrapolated expectation by more than 2 s
    /// (playing) / 0.75 s (paused) is a seek.
    fn sought(prev: &MediaState, cur: &MediaState) -> bool {
        let expected = if prev.playing {
            prev.position_sec + cur.sampled_at.saturating_sub(prev.sampled_at) as f64 / 1000.0
        } else {
            prev.position_sec
        };
        let tolerance = if cur.playing { 2.0 } else { 0.75 };
        (cur.position_sec - expected).abs() > tolerance
    }
}

// ---------------------------------------------------------------------------
// The MTA worker
// ---------------------------------------------------------------------------

#[cfg_attr(not(all(windows, feature = "native")), allow(dead_code))]
enum Request {
    Transport(TransportCommand, Sender<crate::Result<()>>),
    Shutdown,
}

/// Handle to the dedicated GSMTC thread. Dropping it stops the thread.
pub struct MediaWorker {
    tx: Sender<Request>,
}

impl MediaWorker {
    /// Start the worker. `on_change` is called **on the worker thread** with each
    /// meaningful state (see [`MediaTracker`]); keep it quick.
    ///
    /// Returns [`SysError::Unsupported`] in builds without the `native` feature.
    #[cfg(all(windows, feature = "native"))]
    pub fn spawn<F>(on_change: F) -> crate::Result<MediaWorker>
    where
        F: FnMut(Option<MediaState>) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("krovi-media".into())
            .spawn(move || native::run(rx, on_change))
            .map_err(|e| SysError::Message(e.to_string()))?;
        Ok(MediaWorker { tx })
    }

    /// See the native variant.
    #[cfg(not(all(windows, feature = "native")))]
    pub fn spawn<F>(on_change: F) -> crate::Result<MediaWorker>
    where
        F: FnMut(Option<MediaState>) + Send + 'static,
    {
        let _ = on_change;
        let _ = mpsc::channel::<Request>();
        Err(SysError::Unsupported)
    }

    /// Run a transport command on the worker thread and wait (≤ 4 s) for the result.
    pub fn transport(&self, cmd: TransportCommand) -> crate::Result<()> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(Request::Transport(cmd, rtx))
            .map_err(|_| SysError::Message("media worker has stopped".into()))?;
        rrx.recv_timeout(Duration::from_secs(4))
            .map_err(|_| SysError::Message("media worker did not answer".into()))?
    }
}

impl Drop for MediaWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
    }
}

// Keep the receiver type nameable for the native module in all builds.
#[allow(dead_code)]
type RequestRx = Receiver<Request>;

/// One-shot read on a throwaway MTA thread (safe to call from any thread, including
/// the UI thread — it never blocks the caller's apartment). Prefer [`MediaWorker`]
/// for anything periodic.
#[cfg(all(windows, feature = "native"))]
pub fn read_now_playing() -> crate::Result<Option<MediaTrack>> {
    native::read_once().map(|o| o.map(|s| s.as_track()))
}

/// See the native variant. Without `native`, no OS binding is compiled.
#[cfg(not(all(windows, feature = "native")))]
pub fn read_now_playing() -> crate::Result<Option<MediaTrack>> {
    Ok(None)
}

/// One-shot transport command on a throwaway MTA thread.
#[cfg(all(windows, feature = "native"))]
pub fn send_transport(cmd: TransportCommand) -> crate::Result<()> {
    native::transport_once(cmd)
}

/// See the native variant. An action cannot be faked, so it reports unsupported.
#[cfg(not(all(windows, feature = "native")))]
pub fn send_transport(cmd: TransportCommand) -> crate::Result<()> {
    let _ = cmd;
    Err(SysError::Unsupported)
}

// ---------------------------------------------------------------------------
// Native GSMTC implementation. Read-only: NO audio capture, only the metadata
// GSMTC already exposes. Every call here runs on an MTA thread we created.
// ---------------------------------------------------------------------------
#[cfg(all(windows, feature = "native"))]
mod native {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    fn err<E: std::fmt::Display>(e: E) -> SysError {
        SysError::Message(e.to_string())
    }

    /// Join this thread to the multithreaded apartment. `S_FALSE` (already
    /// initialized) and `RPC_E_CHANGED_MODE` are harmless here.
    fn init_mta() {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    }

    fn manager() -> crate::Result<Manager> {
        Manager::RequestAsync().map_err(err)?.get().map_err(err)
    }

    /// Per-track artwork memo: the thumbnail is only re-read when the track changes.
    /// Apps often publish art a few seconds after the text, so a missing thumbnail is
    /// retried every poll at first, then every [`ART_RETRY_EVERY`] polls for a while.
    #[derive(Default)]
    struct ArtCache {
        key: String,
        art: Option<String>,
        polls: u32,
    }

    fn artwork(props: &Props, key: String, cache: &mut ArtCache) -> Option<String> {
        if cache.key != key {
            *cache = ArtCache {
                key,
                art: None,
                polls: 0,
            };
        }
        if cache.art.is_none() {
            let attempt = art_attempt_due(cache.polls);
            cache.polls = cache.polls.saturating_add(1);
            if attempt {
                cache.art = read_thumbnail(props);
            }
        }
        cache.art.clone()
    }

    /// Opt-in diagnostics: set `KROVI_DEBUG_MEDIA=1` to see why artwork is missing.
    fn debug(step: &str, detail: impl std::fmt::Display) {
        if std::env::var_os("KROVI_DEBUG_MEDIA").is_some() {
            eprintln!("[krovi-media] artwork {step}: {detail}");
        }
    }

    fn read_thumbnail(props: &Props) -> Option<String> {
        let reference = props
            .Thumbnail()
            .map_err(|e| debug("no Thumbnail()", e))
            .ok()?;
        let stream = reference
            .OpenReadAsync()
            .and_then(|op| op.get())
            .map_err(|e| debug("OpenReadAsync", e))
            .ok()?;
        let size = stream.Size().map_err(|e| debug("Size", e)).ok()? as usize;
        if size == 0 || size > ARTWORK_READ_MAX_BYTES {
            debug("size out of range", size);
            return None;
        }
        let input = stream
            .GetInputStreamAt(0)
            .map_err(|e| debug("GetInputStreamAt", e))
            .ok()?;
        let reader = DataReader::CreateDataReader(&input)
            .map_err(|e| debug("CreateDataReader", e))
            .ok()?;
        reader
            .LoadAsync(size as u32)
            .and_then(|op| op.get())
            .map_err(|e| debug("LoadAsync", e))
            .ok()?;
        let mut buf = vec![0u8; size];
        reader
            .ReadBytes(&mut buf)
            .map_err(|e| debug("ReadBytes", e))
            .ok()?;
        let art = shrink_artwork(&buf);
        if art.is_none() {
            debug("unusable image bytes", size);
        }
        art
    }

    fn read_state(
        mgr: &Manager,
        art: &mut ArtCache,
        now_ms: u64,
    ) -> crate::Result<Option<MediaState>> {
        let session = match mgr.GetCurrentSession() {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };
        let props = session
            .TryGetMediaPropertiesAsync()
            .map_err(err)?
            .get()
            .map_err(err)?;
        let text = |r: windows::core::Result<windows::core::HSTRING>| {
            r.map(|h| h.to_string_lossy()).unwrap_or_default()
        };
        let title = text(props.Title());
        let artist = text(props.Artist());
        let album = text(props.AlbumTitle());
        if title.is_empty() && artist.is_empty() {
            return Ok(None);
        }
        let source_app = friendly_source(&text(session.SourceAppUserModelId()));

        let info = session.GetPlaybackInfo().ok();
        let playing = info
            .as_ref()
            .and_then(|i| i.PlaybackStatus().ok())
            .map(|s| s == Status::Playing)
            .unwrap_or(false);
        let rate = info
            .as_ref()
            .and_then(|i| i.PlaybackRate().ok())
            .and_then(|r| r.Value().ok())
            .unwrap_or(1.0);

        let (position_sec, duration_sec) = match session.GetTimelineProperties() {
            Ok(tl) => timeline_seconds(
                tl.Position().map(|d| d.Duration).unwrap_or(0),
                tl.StartTime().map(|d| d.Duration).unwrap_or(0),
                tl.EndTime().map(|d| d.Duration).unwrap_or(0),
                tl.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0),
                unix_ms_to_win_ticks(now_ms),
                playing,
                rate,
            ),
            Err(_) => (0.0, 0.0),
        };

        let key = format!("{source_app}|{title}|{artist}|{album}");
        let artwork = artwork(&props, key, art);

        Ok(Some(MediaState {
            title,
            artist,
            album,
            playing,
            position_sec,
            duration_sec,
            sampled_at: now_ms,
            source_app,
            artwork,
        }))
    }

    fn do_transport(mgr: &Manager, cmd: TransportCommand) -> crate::Result<()> {
        let session = mgr.GetCurrentSession().map_err(|_| SysError::Unsupported)?;
        let op = match cmd {
            TransportCommand::Play => session.TryPlayAsync(),
            TransportCommand::Pause => session.TryPauseAsync(),
            TransportCommand::PlayPause => session.TryTogglePlayPauseAsync(),
            TransportCommand::Next => session.TrySkipNextAsync(),
            TransportCommand::Previous => session.TrySkipPreviousAsync(),
        }
        .map_err(err)?;
        if op.get().map_err(err)? {
            Ok(())
        } else {
            Err(SysError::Message(
                "the media app refused the command".into(),
            ))
        }
    }

    pub(super) fn run<F>(rx: Receiver<Request>, mut on_change: F)
    where
        F: FnMut(Option<MediaState>),
    {
        init_mta();
        let mut mgr: Option<Manager> = None;
        let mut tracker = MediaTracker::new();
        let mut art = ArtCache::default();
        let mut quick = 0u8;
        loop {
            if mgr.is_none() {
                mgr = manager().ok();
            }
            let wait = if quick > 0 {
                quick -= 1;
                Duration::from_millis(150)
            } else {
                POLL
            };
            match rx.recv_timeout(wait) {
                Ok(Request::Transport(cmd, reply)) => {
                    let result = match &mgr {
                        Some(m) => do_transport(m, cmd),
                        None => Err(SysError::Unsupported),
                    };
                    let _ = reply.send(result);
                    quick = 3; // re-read soon so the UI reflects the new state fast
                }
                Ok(Request::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = crate::now_ms();
            let sample = mgr
                .as_ref()
                .and_then(|m| read_state(m, &mut art, now).ok())
                .flatten();
            if let Some(payload) = tracker.observe(sample, now) {
                on_change(payload);
            }
        }
    }

    /// Run `f` on a fresh MTA thread and return its result.
    fn on_mta_thread<T: Send + 'static>(
        f: impl FnOnce() -> crate::Result<T> + Send + 'static,
    ) -> crate::Result<T> {
        std::thread::Builder::new()
            .name("krovi-media-once".into())
            .spawn(move || {
                init_mta();
                f()
            })
            .map_err(err)?
            .join()
            .map_err(|_| SysError::Message("media thread panicked".into()))?
    }

    pub(super) fn read_once() -> crate::Result<Option<MediaState>> {
        on_mta_thread(|| {
            let mgr = manager()?;
            read_state(&mgr, &mut ArtCache::default(), crate::now_ms())
        })
    }

    pub(super) fn transport_once(cmd: TransportCommand) -> crate::Result<()> {
        on_mta_thread(move || do_transport(&manager()?, cmd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(title: &str, playing: bool, pos: f64, at: u64) -> MediaState {
        MediaState {
            title: title.into(),
            artist: "Probe".into(),
            album: String::new(),
            playing,
            position_sec: pos,
            duration_sec: 200.0,
            sampled_at: at,
            source_app: "chrome.exe".into(),
            artwork: None,
        }
    }

    #[test]
    fn transport_roundtrips_through_strings() {
        for c in [
            TransportCommand::Play,
            TransportCommand::Pause,
            TransportCommand::PlayPause,
            TransportCommand::Next,
            TransportCommand::Previous,
        ] {
            assert_eq!(TransportCommand::from_str(c.as_str()), Some(c));
        }
    }

    #[test]
    fn transport_parse_is_case_insensitive_with_aliases() {
        assert_eq!(
            TransportCommand::from_str("PLAY"),
            Some(TransportCommand::Play)
        );
        assert_eq!(
            TransportCommand::from_str("prev"),
            Some(TransportCommand::Previous)
        );
        assert_eq!(
            TransportCommand::from_str("skip"),
            Some(TransportCommand::Next)
        );
        assert_eq!(
            TransportCommand::from_str("toggle"),
            Some(TransportCommand::PlayPause)
        );
        assert_eq!(TransportCommand::from_str("eject"), None);
    }

    #[test]
    fn progress_is_clamped() {
        assert_eq!(progress_fraction(0, 0), 0.0);
        assert_eq!(progress_fraction(50, 0), 0.0);
        assert!((progress_fraction(72, 214) - 72.0 / 214.0).abs() < 1e-12);
        assert_eq!(progress_fraction(300, 200), 1.0);
    }

    #[test]
    fn media_track_serializes_camel_case() {
        let t = MediaTrack {
            title: "T".into(),
            artist: "A".into(),
            album: "B".into(),
            playing: true,
            position_sec: 72,
            duration_sec: 214,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"positionSec\":72"));
        assert!(json.contains("\"durationSec\":214"));
    }

    #[test]
    fn media_state_serializes_the_contract_shape() {
        let s = st("Krovi QA A", true, 12.5, 1_700_000_000_000);
        let v = serde_json::to_value(&s).unwrap();
        for k in [
            "title",
            "artist",
            "album",
            "playing",
            "positionSec",
            "durationSec",
            "sampledAt",
            "sourceApp",
            "artwork",
        ] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        assert!(v["artwork"].is_null());
        assert_eq!(v["sampledAt"], 1_700_000_000_000u64);
    }

    #[test]
    fn windows_ticks_conversion() {
        assert_eq!(unix_ms_to_win_ticks(0), 116_444_736_000_000_000);
        assert_eq!(
            unix_ms_to_win_ticks(1000),
            116_444_736_000_000_000 + 10_000_000
        );
    }

    #[test]
    fn timeline_extrapolates_only_while_playing_with_a_duration() {
        let s = 10_000_000i64; // 1 s in ticks
        let now = unix_ms_to_win_ticks(100_000);
        let updated = now - 5 * s; // timeline last updated 5 s ago
                                   // playing: 20 s + 5 s elapsed
        assert_eq!(
            timeline_seconds(20 * s, 0, 200 * s, updated, now, true, 1.0),
            (25.0, 200.0)
        );
        // paused: frozen
        assert_eq!(
            timeline_seconds(20 * s, 0, 200 * s, updated, now, false, 1.0),
            (20.0, 200.0)
        );
        // 2x speed
        assert_eq!(
            timeline_seconds(20 * s, 0, 200 * s, updated, now, true, 2.0).0,
            30.0
        );
        // no duration reported -> no extrapolation
        assert_eq!(
            timeline_seconds(20 * s, 0, 0, updated, now, true, 1.0),
            (20.0, 0.0)
        );
        // never past the end
        assert_eq!(
            timeline_seconds(198 * s, 0, 200 * s, updated, now, true, 1.0).0,
            200.0
        );
        // unset last-updated -> no extrapolation
        assert_eq!(
            timeline_seconds(20 * s, 0, 200 * s, 0, now, true, 1.0).0,
            20.0
        );
        // nonsense rate falls back to 1x
        assert_eq!(
            timeline_seconds(20 * s, 0, 200 * s, updated, now, true, f64::NAN).0,
            25.0
        );
    }

    #[test]
    fn timeline_is_relative_to_start_time() {
        let s = 10_000_000i64;
        assert_eq!(
            timeline_seconds(130 * s, 100 * s, 300 * s, 0, 0, false, 1.0),
            (30.0, 200.0)
        );
    }

    #[test]
    fn advanced_to_moves_only_while_playing() {
        let p = st("A", true, 10.0, 1_000);
        let a = p.advanced_to(4_000);
        assert_eq!(a.position_sec, 13.0);
        assert_eq!(a.sampled_at, 4_000);
        let q = st("A", false, 10.0, 1_000);
        assert_eq!(q.advanced_to(9_000).position_sec, 10.0);
        // clamps to the duration
        assert_eq!(
            st("A", true, 199.0, 0).advanced_to(10_000).position_sec,
            200.0
        );
        // time going backwards is a no-op
        assert_eq!(p.advanced_to(500).position_sec, 10.0);
    }

    #[test]
    fn source_app_cleanup() {
        assert_eq!(friendly_source("Spotify.exe"), "Spotify.exe");
        assert_eq!(friendly_source("  chrome.exe "), "chrome.exe");
        assert_eq!(
            friendly_source("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "Microsoft.ZuneMusic"
        );
        assert_eq!(friendly_source("weird!name"), "weird!name"); // no package marker
        assert_eq!(friendly_source(""), "");
    }

    #[test]
    fn artwork_passes_small_images_through_and_shrinks_big_pngs() {
        // small jpeg-ish blob passes through with its sniffed mime
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        let url = shrink_artwork(&jpeg).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        // unknown bytes are dropped
        assert_eq!(shrink_artwork(b"not an image"), None);
        // an oversize non-PNG is dropped
        let mut big_jpeg = vec![0xFF, 0xD8, 0xFF];
        big_jpeg.resize(ARTWORK_CAP_BYTES + 1, 7);
        assert_eq!(shrink_artwork(&big_jpeg), None);
        // an oversize PNG is re-encoded under the cap
        let (w, h) = (700u32, 700u32);
        let mut seed = 0x9E3779B9u32;
        let mut rgba = Vec::new();
        for _ in 0..(w * h) {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            rgba.extend_from_slice(&[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 255]);
        }
        let png = imaging::encode_png(w, h, &rgba).unwrap();
        assert!(png.len() > ARTWORK_CAP_BYTES);
        let url = shrink_artwork(&png).unwrap();
        let (mime, bytes) = imaging::parse_data_url(&url).unwrap();
        assert_eq!(mime, "image/png");
        assert!(bytes.len() <= ARTWORK_CAP_BYTES);
    }

    #[test]
    fn artwork_retries_fast_then_slow_then_stops() {
        assert!((0..ART_FAST_POLLS).all(art_attempt_due));
        assert!(!art_attempt_due(ART_FAST_POLLS + 1));
        assert!(art_attempt_due(ART_RETRY_EVERY * 2));
        assert!(!art_attempt_due(ART_GIVE_UP_POLLS));
        assert!(!art_attempt_due(ART_GIVE_UP_POLLS + 10));
    }

    #[test]
    fn tracker_emits_first_sample_even_when_nothing_is_playing() {
        let mut t = MediaTracker::new();
        assert_eq!(t.observe(None, 0), Some(None));
        assert_eq!(t.observe(None, 500), None);
        assert_eq!(t.observe(None, 1_000), None);
    }

    #[test]
    fn tracker_emits_on_track_and_state_changes() {
        let mut t = MediaTracker::new();
        let a = st("Krovi QA A", true, 5.0, 0);
        assert!(t.observe(Some(a.clone()), 0).is_some());
        // same track, position advanced exactly as expected -> quiet
        assert_eq!(t.observe(Some(st("Krovi QA A", true, 5.5, 500)), 500), None);
        // new title -> emit
        let b = st("Krovi QA B", true, 0.0, 1_000);
        assert_eq!(t.observe(Some(b.clone()), 1_000), Some(Some(b)));
        // pause -> emit
        let paused = st("Krovi QA B", false, 0.5, 1_500);
        assert_eq!(t.observe(Some(paused.clone()), 1_500), Some(Some(paused)));
        // session disappears -> emit null once
        assert_eq!(t.observe(None, 2_000), Some(None));
        assert_eq!(t.observe(None, 2_500), None);
        // and comes back -> emit
        assert!(t
            .observe(Some(st("Krovi QA B", true, 1.0, 3_000)), 3_000)
            .is_some());
    }

    #[test]
    fn tracker_detects_seeks_and_resyncs_while_playing() {
        let mut t = MediaTracker::new();
        assert!(t.observe(Some(st("A", true, 10.0, 0)), 0).is_some());
        // 0.5 s later at 10.5: on track -> quiet
        assert_eq!(t.observe(Some(st("A", true, 10.5, 500)), 500), None);
        // jump to 120 s: a seek -> emit
        assert!(t
            .observe(Some(st("A", true, 120.0, 1_000)), 1_000)
            .is_some());
        // steady playback for 4.5 s (position tracks the clock) -> quiet
        let mut now = 1_000u64;
        let mut pos = 120.0;
        for _ in 0..9 {
            now += 500;
            pos += 0.5;
            assert_eq!(t.observe(Some(st("A", true, pos, now)), now), None);
        }
        // crossing RESYNC_MS since the last emit -> emit once
        now += 500;
        pos += 0.5;
        assert!(t.observe(Some(st("A", true, pos, now)), now).is_some());
        now += 500;
        pos += 0.5;
        assert_eq!(t.observe(Some(st("A", true, pos, now)), now), None);
    }

    #[test]
    fn tracker_does_not_resync_while_paused() {
        let mut t = MediaTracker::new();
        assert!(t.observe(Some(st("A", false, 10.0, 0)), 0).is_some());
        for i in 1..40u64 {
            assert_eq!(
                t.observe(Some(st("A", false, 10.0, i * 500)), i * 500),
                None
            );
        }
    }

    #[test]
    fn tracker_detects_a_paused_scrub() {
        let mut t = MediaTracker::new();
        assert!(t.observe(Some(st("A", false, 10.0, 0)), 0).is_some());
        assert!(t.observe(Some(st("A", false, 42.0, 500)), 500).is_some());
    }

    #[test]
    fn tracker_notices_artwork_arriving_late() {
        let mut t = MediaTracker::new();
        let mut with_art = st("A", true, 1.0, 500);
        assert!(t.observe(Some(st("A", true, 0.5, 0)), 0).is_some());
        with_art.artwork = Some("data:image/png;base64,AAAA".into());
        assert!(t.observe(Some(with_art), 500).is_some());
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn reads_are_noops_without_native() {
        assert_eq!(read_now_playing().unwrap(), None);
        assert!(matches!(
            send_transport(TransportCommand::Play),
            Err(SysError::Unsupported)
        ));
        assert!(matches!(
            MediaWorker::spawn(|_| {}),
            Err(SysError::Unsupported)
        ));
    }
}
