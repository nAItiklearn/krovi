//! Windows side of read-aloud: the on-device voices (WinRT `SpeechSynthesizer`, which lists
//! every installed voice including "Natural" ones when present) and playback of audio that only
//! ever exists in memory (`MediaPlayer` over an `InMemoryRandomAccessStream`). No network, no
//! files. ElevenLabs audio (MP3) and Windows audio (WAV) go through the same player.

use std::sync::atomic::AtomicBool;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WinVoice {
    pub id: String,
    pub name: String,
    pub language: String,
    pub gender: String,
    /// A "Natural" / neural voice (better sounding), when Windows offers one.
    pub natural: bool,
}

/// Encoded audio in memory.
pub struct Audio {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

#[cfg(all(windows, feature = "native"))]
mod imp {
    use std::sync::atomic::Ordering;
    use std::sync::{mpsc, OnceLock};
    use std::time::{Duration, Instant};

    use windows::core::{IInspectable, HSTRING};
    use windows::Foundation::TypedEventHandler;
    use windows::Media::Core::MediaSource;
    use windows::Media::Playback::{MediaPlayer, MediaPlayerFailedEventArgs};
    use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender, VoiceInformation};
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    use super::*;

    fn err(e: windows::core::Error) -> String {
        format!("{} (0x{:08X})", e.message().trim(), e.code().0 as u32)
    }

    static KEEPER: OnceLock<()> = OnceLock::new();

    /// WinRT calls here block on async operations, which must happen on a multithreaded-apartment
    /// thread. Every thread that touches this module calls this first.
    ///
    /// The process's MTA only exists while at least one thread is in it. Our worker threads are
    /// short-lived, and tearing the apartment down while the speech stack still holds state in it
    /// crashes the next WinRT call (an access violation, reproducible). A parked keeper thread
    /// holds the MTA open for the life of the process.
    pub fn init_mta() {
        KEEPER.get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("krovi-explain-mta".into())
                .spawn(move || {
                    // SAFETY: plain apartment initialisation on this thread, never uninitialised.
                    unsafe {
                        let _ = RoInitialize(RO_INIT_MULTITHREADED);
                    }
                    let _ = tx.send(());
                    loop {
                        std::thread::park();
                    }
                });
            let _ = rx.recv_timeout(Duration::from_secs(2));
        });
        // Already-initialised (S_FALSE) and a different mode (RPC_E_CHANGED_MODE) are both fine.
        // SAFETY: as above.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
    }

    fn run_mta<T: Send + 'static>(
        f: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        std::thread::Builder::new()
            .name("krovi-explain-winrt".into())
            .spawn(move || {
                init_mta();
                f()
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "The Windows speech service stopped unexpectedly.".to_string())?
    }

    fn describe(v: &VoiceInformation) -> Result<WinVoice, String> {
        let name = v.DisplayName().map_err(err)?.to_string();
        let natural = name.contains("Natural") || name.contains("Neural");
        Ok(WinVoice {
            id: v.Id().map_err(err)?.to_string(),
            name,
            language: v.Language().map(|s| s.to_string()).unwrap_or_default(),
            gender: match v.Gender() {
                Ok(VoiceGender::Female) => "female",
                Ok(VoiceGender::Male) => "male",
                _ => "",
            }
            .to_string(),
            natural,
        })
    }

    /// Installed on-device voices, Natural ones first.
    pub fn voices() -> Result<Vec<WinVoice>, String> {
        run_mta(|| {
            let all = SpeechSynthesizer::AllVoices().map_err(err)?;
            let mut out = Vec::new();
            for v in all {
                if let Ok(d) = describe(&v) {
                    out.push(d);
                }
            }
            out.sort_by(|a, b| {
                b.natural
                    .cmp(&a.natural)
                    .then(a.language.cmp(&b.language))
                    .then(a.name.cmp(&b.name))
            });
            Ok(out)
        })
    }

    pub fn open_url(url: &str) -> Result<(), String> {
        use windows::core::w;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        // SAFETY: a plain ShellExecute "open" of a URL that was checked against the allow-list.
        let h = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(url),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        // ShellExecute reports success with a value greater than 32.
        if h.0 as isize > 32 {
            Ok(())
        } else {
            Err("Windows could not open your browser.".into())
        }
    }

    /// Id of the voice Windows would use by default.
    pub fn default_voice_id() -> Option<String> {
        run_mta(|| {
            SpeechSynthesizer::DefaultVoice()
                .and_then(|v| v.Id())
                .map(|s| s.to_string())
                .map_err(err)
        })
        .ok()
    }

    /// A synthesizer bound to one voice and speaking rate. Create and use it on one MTA thread.
    pub struct Synth {
        inner: SpeechSynthesizer,
        pub voice: String,
    }

    impl Synth {
        pub fn new(voice_id: Option<&str>, rate: f32) -> Result<Synth, String> {
            let inner = SpeechSynthesizer::new().map_err(err)?;
            if let Some(id) = voice_id {
                if let Ok(all) = SpeechSynthesizer::AllVoices() {
                    for v in all {
                        if v.Id().map(|s| s.to_string()).unwrap_or_default() == id {
                            let _ = inner.SetVoice(&v);
                            break;
                        }
                    }
                }
            }
            if let Ok(o) = inner.Options() {
                let _ = o.SetSpeakingRate(f64::from(rate.clamp(0.5, 3.0)));
            }
            let voice = inner
                .Voice()
                .and_then(|v| v.DisplayName())
                .map(|s| s.to_string())
                .unwrap_or_else(|_| "Windows voice".into());
            Ok(Synth { inner, voice })
        }

        pub fn synth(&self, text: &str) -> Result<Audio, String> {
            let stream = self
                .inner
                .SynthesizeTextToStreamAsync(&HSTRING::from(text))
                .map_err(err)?
                .get()
                .map_err(err)?;
            let size = stream.Size().map_err(err)? as u32;
            let input = stream.GetInputStreamAt(0).map_err(err)?;
            let reader = DataReader::CreateDataReader(&input).map_err(err)?;
            reader.LoadAsync(size).map_err(err)?.get().map_err(err)?;
            let mut bytes = vec![0u8; size as usize];
            reader.ReadBytes(&mut bytes).map_err(err)?;
            Ok(Audio {
                bytes,
                mime: "audio/wav",
            })
        }
    }

    /// Play `audio` through the default output device and block until it ends, fails, or
    /// `cancel` is set. Nothing is written to disk.
    pub fn play(audio: &Audio, cancel: &AtomicBool) -> Result<(), String> {
        let stream = InMemoryRandomAccessStream::new().map_err(err)?;
        {
            let writer = DataWriter::CreateDataWriter(&stream).map_err(err)?;
            writer.WriteBytes(&audio.bytes).map_err(err)?;
            writer.StoreAsync().map_err(err)?.get().map_err(err)?;
            writer.FlushAsync().map_err(err)?.get().map_err(err)?;
            writer.DetachStream().map_err(err)?;
        }
        stream.Seek(0).map_err(err)?;
        let source =
            MediaSource::CreateFromStream(&stream, &HSTRING::from(audio.mime)).map_err(err)?;
        let player = MediaPlayer::new().map_err(err)?;
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let tx_end = tx.clone();
        player
            .MediaEnded(&TypedEventHandler::<MediaPlayer, IInspectable>::new(
                move |_, _| {
                    let _ = tx_end.send(Ok(()));
                    Ok(())
                },
            ))
            .map_err(err)?;
        player
            .MediaFailed(
                &TypedEventHandler::<MediaPlayer, MediaPlayerFailedEventArgs>::new(
                    move |_, args| {
                        let msg = args
                            .as_ref()
                            .and_then(|a| a.ErrorMessage().ok())
                            .map(|s| s.to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Windows could not play this audio.".into());
                        let _ = tx.send(Err(msg));
                        Ok(())
                    },
                ),
            )
            .map_err(err)?;
        player.SetSource(&source).map_err(err)?;
        player.Play().map_err(err)?;

        // A player that never reports back must not hang the app: allow a generous window
        // based on the size of the audio (>= 8 kB/s covers MP3 at 64 kbps and 16-bit WAV).
        let deadline =
            Instant::now() + Duration::from_secs(30 + (audio.bytes.len() / 8_000) as u64);
        let result = loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(r) => break r,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if cancel.load(Ordering::SeqCst) {
                        let _ = player.Pause();
                        break Ok(());
                    }
                    if Instant::now() > deadline {
                        break Err("Windows audio did not finish playing.".into());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        let _ = player.Close();
        result
    }
}

#[cfg(not(all(windows, feature = "native")))]
mod imp {
    use super::*;

    const NO: &str = "Read aloud needs Windows.";
    pub fn init_mta() {}
    pub fn voices() -> Result<Vec<WinVoice>, String> {
        Err(NO.into())
    }
    pub fn default_voice_id() -> Option<String> {
        None
    }
    pub fn open_url(_: &str) -> Result<(), String> {
        Err(NO.into())
    }
    pub struct Synth {
        pub voice: String,
    }
    impl Synth {
        pub fn new(_: Option<&str>, _: f32) -> Result<Synth, String> {
            Err(NO.into())
        }
        pub fn synth(&self, _: &str) -> Result<Audio, String> {
            Err(NO.into())
        }
    }
    pub fn play(_: &Audio, _: &AtomicBool) -> Result<(), String> {
        Err(NO.into())
    }
}

pub use imp::{default_voice_id, init_mta, play, voices, Synth};

/// Open `url` in the user's default browser - but only the one address Krovi ever offers (the
/// ElevenLabs API-keys page). The webview cannot use this to open anything else.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    if url != crate::eleven::KEYS_PAGE_URL {
        return Err("That address isn't one Krovi opens.".into());
    }
    imp::open_url(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_fixed_keys_page_can_be_opened() {
        for bad in [
            "https://example.com",
            "file:///C:/Windows/System32/calc.exe",
            "calc.exe",
            "https://elevenlabs.io/app/settings/api-keys?x=1",
            "",
        ] {
            assert!(open_in_browser(bad).is_err(), "{bad}");
        }
    }
}
