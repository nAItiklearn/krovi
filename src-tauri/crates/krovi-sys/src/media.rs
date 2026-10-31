//! System media now-playing + transport (SPEC C5).
//!
//! Reads the Windows `GlobalSystemMediaTransportControlsSessionManager` (GSMTC)
//! current session — the same track metadata the OS shows on the media flyout —
//! and can issue transport commands (play/pause/next/prev). It is strictly
//! **read-only with respect to content**: it captures **no audio**, only the
//! title/artist/album/position the system already publishes.
//!
//! The [`MediaTrack`] DTO and the [`TransportCommand`] mapping are pure and
//! tested. The GSMTC calls are isolated in the `native` submodule (CI-only).

use serde::Serialize;

use crate::SysError;

/// What is currently playing, as the system reports it. Times are whole seconds.
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

/// Read the current media session, if any.
///
/// Returns `Ok(None)` when there is no session **and** in any build without the
/// `native` feature (so the browser/demo build is a harmless no-op). The real
/// GSMTC read happens only under `native` on Windows.
#[cfg(all(windows, feature = "native"))]
pub fn read_now_playing() -> crate::Result<Option<MediaTrack>> {
    native::read_now_playing()
}

/// See the native variant. Without `native`, no OS binding is compiled.
#[cfg(not(all(windows, feature = "native")))]
pub fn read_now_playing() -> crate::Result<Option<MediaTrack>> {
    Ok(None)
}

/// Send a transport command to the active session.
///
/// Returns [`SysError::Unsupported`] in any build without the `native` feature —
/// an action cannot be faked, so it is reported as unavailable rather than
/// silently succeeding.
#[cfg(all(windows, feature = "native"))]
pub fn send_transport(cmd: TransportCommand) -> crate::Result<()> {
    native::send_transport(cmd)
}

/// See the native variant. Without `native`, no OS binding is compiled.
#[cfg(not(all(windows, feature = "native")))]
pub fn send_transport(cmd: TransportCommand) -> crate::Result<()> {
    let _ = cmd;
    Err(SysError::Unsupported)
}

// ---------------------------------------------------------------------------
// Native GSMTC implementation — CI-only.
//
// Authored against windows-rs 0.58. Not compiled or exercised on the local
// 32-bit-gcc host; no local test results are claimed. Read-only: NO audio
// capture, only the metadata GSMTC already exposes.
// ---------------------------------------------------------------------------
#[cfg(all(windows, feature = "native"))]
mod native {
    use super::{MediaTrack, TransportCommand};
    use crate::SysError;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    fn err<E: std::fmt::Display>(e: E) -> SysError {
        SysError::Message(e.to_string())
    }

    fn manager() -> crate::Result<Manager> {
        Manager::RequestAsync().map_err(err)?.get().map_err(err)
    }

    pub(super) fn read_now_playing() -> crate::Result<Option<MediaTrack>> {
        let mgr = manager()?;
        let session = match mgr.GetCurrentSession() {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };
        let props = session
            .TryGetMediaPropertiesAsync()
            .map_err(err)?
            .get()
            .map_err(err)?;
        let title = props
            .Title()
            .map(|h| h.to_string_lossy())
            .unwrap_or_default();
        let artist = props
            .Artist()
            .map(|h| h.to_string_lossy())
            .unwrap_or_default();
        let album = props
            .AlbumTitle()
            .map(|h| h.to_string_lossy())
            .unwrap_or_default();

        let playing = session
            .GetPlaybackInfo()
            .ok()
            .and_then(|i| i.PlaybackStatus().ok())
            .map(|s| s == Status::Playing)
            .unwrap_or(false);

        // Windows TimeSpan is in 100ns ticks → seconds.
        let (position_sec, duration_sec) = match session.GetTimelineProperties() {
            Ok(tl) => {
                let pos = tl
                    .Position()
                    .map(|d| (d.Duration / 10_000_000) as u64)
                    .unwrap_or(0);
                let end = tl
                    .EndTime()
                    .map(|d| (d.Duration / 10_000_000) as u64)
                    .unwrap_or(0);
                (pos, end)
            }
            Err(_) => (0, 0),
        };

        Ok(Some(MediaTrack {
            title,
            artist,
            album,
            playing,
            position_sec,
            duration_sec,
        }))
    }

    pub(super) fn send_transport(cmd: TransportCommand) -> crate::Result<()> {
        let mgr = manager()?;
        let session = mgr.GetCurrentSession().map_err(|_| SysError::Unsupported)?;
        let op = match cmd {
            TransportCommand::Play => session.TryPlayAsync(),
            TransportCommand::Pause => session.TryPauseAsync(),
            TransportCommand::PlayPause => session.TryTogglePlayPauseAsync(),
            TransportCommand::Next => session.TrySkipNextAsync(),
            TransportCommand::Previous => session.TrySkipPreviousAsync(),
        }
        .map_err(err)?;
        op.get().map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            title: "Midnight Drive".into(),
            artist: "Neon Harbor".into(),
            album: "After Hours".into(),
            playing: true,
            position_sec: 72,
            duration_sec: 214,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"positionSec\":72"));
        assert!(json.contains("\"durationSec\":214"));
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn reads_are_noops_without_native() {
        assert_eq!(read_now_playing().unwrap(), None);
        assert!(matches!(
            send_transport(TransportCommand::Play),
            Err(SysError::Unsupported)
        ));
    }
}
