//! Read text aloud with the Windows on-device voice, headless, and PROVE it made sound.
//!
//!   cargo run -p krovi-explain --example speak -- [--list] [--voice <name part>] [--rate 1.0]
//!                                                 [--file <path> | <text...>]
//!
//! It logs the voice used, every sentence range as it is spoken and the wall-clock duration, and
//! it samples the Windows audio peak meters (the default output device, and this process's own
//! audio session) while speaking, so "the API returned Ok" is not the only evidence.
//! No network is used by this example.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use krovi_explain::speak::{Engine, SpeakEvent, SpeakRequest, SpeakSink, Speaker};
use krovi_explain::win;

struct Log {
    t0: Instant,
    tx: mpsc::Sender<SpeakEvent>,
}

impl SpeakSink for Log {
    fn event(&self, ev: SpeakEvent) {
        println!("[{:>6} ms] {:?}", self.t0.elapsed().as_millis(), ev);
        let _ = self.tx.send(ev);
    }
}

#[cfg(windows)]
mod meter {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use windows::core::Interface;
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    #[derive(Debug, Default, Clone, Copy)]
    pub struct Peaks {
        pub samples: usize,
        pub endpoint_max: f32,
        pub own_max: f32,
        pub own_active_samples: usize,
    }

    pub fn start(stop: Arc<AtomicBool>, pid: u32) -> JoinHandle<Peaks> {
        std::thread::spawn(move || {
            let mut p = Peaks::default();
            // SAFETY: standard COM setup/use on this thread only.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(
                    &MMDeviceEnumerator,
                    None,
                    CLSCTX_ALL,
                ) else {
                    return p;
                };
                let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) else {
                    return p;
                };
                let endpoint: Option<IAudioMeterInformation> = dev.Activate(CLSCTX_ALL, None).ok();
                let mgr: Option<IAudioSessionManager2> = dev.Activate(CLSCTX_ALL, None).ok();
                while !stop.load(Ordering::SeqCst) {
                    p.samples += 1;
                    if let Some(e) = &endpoint {
                        if let Ok(v) = e.GetPeakValue() {
                            p.endpoint_max = p.endpoint_max.max(v);
                        }
                    }
                    if let Some(m) = &mgr {
                        if let Ok(list) = m.GetSessionEnumerator() {
                            let n = list.GetCount().unwrap_or(0);
                            for i in 0..n {
                                let Ok(ctl) = list.GetSession(i) else {
                                    continue;
                                };
                                let Ok(c2) = ctl.cast::<IAudioSessionControl2>() else {
                                    continue;
                                };
                                if c2.GetProcessId().unwrap_or(0) != pid {
                                    continue;
                                }
                                if let Ok(meter) = ctl.cast::<IAudioMeterInformation>() {
                                    let v = meter.GetPeakValue().unwrap_or(0.0);
                                    p.own_max = p.own_max.max(v);
                                    if v > 0.001 {
                                        p.own_active_samples += 1;
                                    }
                                }
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
            p
        })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut voice_part: Option<String> = None;
    let mut rate = 1.0f32;
    let mut text: Vec<String> = Vec::new();
    let mut file: Option<String> = None;
    let mut list = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => list = true,
            "--voice" => voice_part = it.next().cloned(),
            "--rate" => rate = it.next().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            "--file" => file = it.next().cloned(),
            other => text.push(other.to_string()),
        }
    }

    let voices = match win::voices() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot list Windows voices: {e}");
            std::process::exit(1);
        }
    };
    println!("{} Windows voices installed:", voices.len());
    for v in &voices {
        println!(
            "  {}{} [{}, {}]  id={}",
            if v.natural { "* " } else { "  " },
            v.name,
            v.language,
            v.gender,
            v.id
        );
    }
    println!("  (default voice id: {:?})", win::default_voice_id());
    if list {
        return;
    }

    let voice_id = voice_part.as_ref().and_then(|p| {
        voices
            .iter()
            .find(|v| v.name.to_lowercase().contains(&p.to_lowercase()))
            .map(|v| v.id.clone())
    });
    let body = match file {
        Some(p) => std::fs::read_to_string(p).expect("cannot read file"),
        None if !text.is_empty() => text.join(" "),
        None => "Krovi can read a summary aloud. This sentence is spoken by a Windows voice that lives on this computer. Nothing is sent over the network.".to_string(),
    };

    let (tx, rx) = mpsc::channel();
    let sink = Arc::new(Log {
        t0: Instant::now(),
        tx,
    });
    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(windows)]
    let meter = meter::start(stop.clone(), std::process::id());
    std::thread::sleep(std::time::Duration::from_millis(300)); // baseline before speaking

    let speaker = Speaker::new();
    let started = Instant::now();
    let info = speaker
        .speak(
            SpeakRequest {
                text: body.clone(),
                engine: Engine::Windows { voice_id },
                rate,
            },
            sink,
        )
        .expect("speak failed to start");
    println!("started: {info:?}");

    let mut outcome = String::from("(no outcome)");
    let mut voice_used = String::new();
    while let Ok(ev) = rx.recv() {
        match ev {
            SpeakEvent::Speaking { voice, .. } => voice_used = voice,
            SpeakEvent::Done { ms } => {
                outcome = format!("done in {ms} ms");
                break;
            }
            SpeakEvent::Failed { message } => {
                outcome = format!("FAILED: {message}");
                break;
            }
            SpeakEvent::Stopped => {
                outcome = "stopped".into();
                break;
            }
            SpeakEvent::Notice { .. } => {}
        }
    }
    let wall = started.elapsed();
    std::thread::sleep(std::time::Duration::from_millis(300));
    stop.store(true, Ordering::SeqCst);

    println!("---- result");
    println!("voice used   : {voice_used}");
    println!(
        "characters   : {}  sentences: {}",
        info.chars, info.sentences
    );
    println!("outcome      : {outcome}");
    println!("wall duration: {:.2} s (rate {rate})", wall.as_secs_f64());
    #[cfg(windows)]
    {
        let p = meter.join().unwrap_or_default();
        println!("audio meter  : {} samples; default output peak max {:.3}; THIS PROCESS's audio session peak max {:.3}, active in {} samples ({:.1} s)", p.samples, p.endpoint_max, p.own_max, p.own_active_samples, p.own_active_samples as f64 * 0.025);
        println!(
            "sound proof  : {}",
            if p.own_max > 0.001 {
                "YES - Windows reports audio coming out of this process"
            } else {
                "NO - nothing measurable came out of this process"
            }
        );
    }
}
