//! Microphone capture for local dictation: WASAPI shared mode, 16 kHz mono, on-device.
//!
//! * The microphone is opened only by [`Recorder::start`] (an explicit user action) and
//!   closed when the recorder is stopped, cancelled or dropped. Nothing listens otherwise.
//! * Audio stays in this process's memory and is handed to the local recognizer. It is
//!   never written to disk and never sent anywhere.
//! * Windows converts the device's mix format to 16 kHz mono 16-bit PCM for us
//!   (`AUTOCONVERTPCM | SRC_DEFAULT_QUALITY`), so no resampler is needed.
//! * [`list_input_devices`] enumerates active capture endpoints (id + friendly name) for
//!   the microphone picker; [`Recorder::level`] gives a live RMS for the mic test meter.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

use crate::vad::SAMPLE_RATE;

/// Hard ceiling on one recording (the dictation target is 20 minutes).
pub const MAX_RECORDING: Duration = Duration::from_secs(21 * 60);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    /// Opaque endpoint id (stable across reboots), for [`Recorder::start`].
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

const E_NOTFOUND: i32 = 0x8007_0490_u32 as i32;

fn hr_message(e: &windows::core::Error) -> String {
    const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
    const AUDCLNT_E_DEVICE_INVALIDATED: i32 = 0x8889_0004_u32 as i32;
    const AUDCLNT_E_DEVICE_IN_USE: i32 = 0x8889_000A_u32 as i32;
    match e.code().0 {
        E_ACCESSDENIED => "Windows is blocking microphone access. Turn on Settings > Privacy & security > Microphone > Let desktop apps access your microphone.".into(),
        AUDCLNT_E_DEVICE_INVALIDATED => "The microphone was unplugged or disabled.".into(),
        AUDCLNT_E_DEVICE_IN_USE => "Another app is using the microphone exclusively.".into(),
        E_NOTFOUND => "That microphone is not connected.".into(),
        _ => format!("Microphone error: {}", e.message()),
    }
}

/// COM for the calling thread, released on drop.
struct Com;
impl Com {
    fn init() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|e| format!("COM init failed: {}", e.message()))?;
        Ok(Com)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

unsafe fn take_pwstr(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| hr_message(&e))
}

fn device_id(d: &IMMDevice) -> Result<String, String> {
    unsafe { d.GetId().map(|p| take_pwstr(p)) }.map_err(|e| hr_message(&e))
}

fn device_name(d: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = d.OpenPropertyStore(STGM_READ) else {
            return "Microphone".into();
        };
        let Ok(value) = store.GetValue(&PKEY_Device_FriendlyName) else {
            return "Microphone".into();
        };
        match PropVariantToStringAlloc(&value) {
            Ok(p) => take_pwstr(p),
            Err(_) => "Microphone".into(),
        }
    }
}

/// Active microphones, default first. Opens no audio stream.
pub fn list_input_devices() -> Result<Vec<InputDevice>, String> {
    let _com = Com::init()?;
    let en = enumerator()?;
    let default_id = unsafe { en.GetDefaultAudioEndpoint(eCapture, eConsole) }
        .ok()
        .and_then(|d| device_id(&d).ok());
    let coll = unsafe { en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }
        .map_err(|e| hr_message(&e))?;
    let n = unsafe { coll.GetCount() }.map_err(|e| hr_message(&e))?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let Ok(d) = (unsafe { coll.Item(i) }) else {
            continue;
        };
        let Ok(id) = device_id(&d) else { continue };
        out.push(InputDevice {
            is_default: default_id.as_deref() == Some(id.as_str()),
            name: device_name(&d),
            id,
        });
    }
    out.sort_by_key(|d| !d.is_default);
    Ok(out)
}

fn open_device(en: &IMMDeviceEnumerator, id: Option<&str>) -> Result<IMMDevice, String> {
    match id {
        Some(id) if !id.is_empty() => {
            let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { en.GetDevice(PCWSTR(wide.as_ptr())) }.map_err(|e| hr_message(&e))
        }
        _ => unsafe { en.GetDefaultAudioEndpoint(eCapture, eConsole) }.map_err(|e| {
            if e.code().0 == E_NOTFOUND {
                "No microphone is connected.".into()
            } else {
                hr_message(&e)
            }
        }),
    }
}

/// One live recording. Stop it to get the audio; drop or cancel it to discard.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    level_bits: Arc<AtomicU32>,
    samples: Arc<Mutex<Vec<i16>>>,
    worker: Option<JoinHandle<Result<(), String>>>,
    started: Instant,
}

impl Recorder {
    /// Open the microphone (`device_id` from [`list_input_devices`], `None` = Windows
    /// default) and start recording. Fails fast with a readable message when Windows
    /// blocks the microphone or the device is gone.
    pub fn start(device_id: Option<String>) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let level_bits = Arc::new(AtomicU32::new(0));
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (s, l, sh) = (stop.clone(), level_bits.clone(), samples.clone());
        let worker = std::thread::Builder::new()
            .name("krovi-mic".into())
            .spawn(move || run(device_id, s, l, sh, ready_tx))
            .map_err(|e| format!("could not start the recorder: {e}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                level_bits,
                samples,
                worker: Some(worker),
                started: Instant::now(),
            }),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err("The microphone did not respond.".into())
            }
        }
    }

    /// Live input level (RMS, 0.0..=1.0) of the most recent audio packet.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level_bits.load(Ordering::Relaxed))
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Number of samples captured so far.
    pub fn len(&self) -> usize {
        self.samples.lock().map(|s| s.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copy of the samples from `from` on (f32), for live end-of-speech detection.
    pub fn tail_since(&self, from: usize) -> Vec<f32> {
        self.samples
            .lock()
            .map(|s| {
                s.get(from..)
                    .unwrap_or(&[])
                    .iter()
                    .map(|&x| f32::from(x) / 32_768.0)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the worker ended on its own (device lost or the length cap hit).
    pub fn finished(&self) -> bool {
        self.worker.as_ref().map_or(true, |w| w.is_finished())
    }

    /// Stop and return everything recorded (16 kHz mono f32).
    pub fn stop(mut self) -> Result<Vec<f32>, String> {
        self.stop.store(true, Ordering::SeqCst);
        let res = self
            .worker
            .take()
            .map(|w| {
                w.join()
                    .unwrap_or_else(|_| Err("the recorder crashed".into()))
            })
            .unwrap_or(Ok(()));
        let raw = std::mem::take(
            &mut *self
                .samples
                .lock()
                .map_err(|_| "recorder state lost".to_string())?,
        );
        if raw.is_empty() {
            res?;
        }
        Ok(raw.into_iter().map(|x| f32::from(x) / 32_768.0).collect())
    }

    /// Stop and throw the audio away.
    pub fn cancel(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        if let Ok(mut s) = self.samples.lock() {
            s.clear();
            s.shrink_to_fit();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

fn setup(device_id: Option<&str>) -> Result<(IAudioClient, IAudioCaptureClient), String> {
    let en = enumerator()?;
    let dev = open_device(&en, device_id)?;
    let client: IAudioClient =
        unsafe { dev.Activate(CLSCTX_ALL, None) }.map_err(|e| hr_message(&e))?;
    let fmt = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE as u32,
        nAvgBytesPerSec: SAMPLE_RATE as u32 * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            2_000_000, // 200 ms buffer, in 100 ns units
            0,
            &fmt,
            None,
        )
    }
    .map_err(|e| hr_message(&e))?;
    let capture: IAudioCaptureClient =
        unsafe { client.GetService() }.map_err(|e| hr_message(&e))?;
    unsafe { client.Start() }.map_err(|e| hr_message(&e))?;
    Ok((client, capture))
}

fn run(
    device_id: Option<String>,
    stop: Arc<AtomicBool>,
    level_bits: Arc<AtomicU32>,
    samples: Arc<Mutex<Vec<i16>>>,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let _com = match Com::init() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let (client, capture) = match setup(device_id.as_deref()) {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let result = pump(&capture, &stop, &level_bits, &samples);
    let _ = unsafe { client.Stop() };
    result
}

fn pump(
    capture: &IAudioCaptureClient,
    stop: &AtomicBool,
    level_bits: &AtomicU32,
    samples: &Mutex<Vec<i16>>,
) -> Result<(), String> {
    let started = Instant::now();
    let max_samples = MAX_RECORDING.as_secs() as usize * SAMPLE_RATE;
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
        let mut packet = unsafe { capture.GetNextPacketSize() }.map_err(|e| hr_message(&e))?;
        while packet > 0 {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }
                .map_err(|e| hr_message(&e))?;
            let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
            let chunk: Vec<i16> = if silent || data.is_null() {
                vec![0; frames as usize]
            } else {
                unsafe { std::slice::from_raw_parts(data as *const i16, frames as usize) }.to_vec()
            };
            let _ = unsafe { capture.ReleaseBuffer(frames) };
            let sumsq: f64 = chunk
                .iter()
                .map(|&s| (f64::from(s) / 32_768.0).powi(2))
                .sum();
            let rms = if chunk.is_empty() {
                0.0
            } else {
                (sumsq / chunk.len() as f64).sqrt() as f32
            };
            level_bits.store(rms.to_bits(), Ordering::Relaxed);
            let full = match samples.lock() {
                Ok(mut s) => {
                    s.extend_from_slice(&chunk);
                    s.len() >= max_samples
                }
                Err(_) => return Err("recorder state lost".into()),
            };
            if full {
                return Ok(());
            }
            packet = unsafe { capture.GetNextPacketSize() }.map_err(|e| hr_message(&e))?;
        }
        if started.elapsed() > MAX_RECORDING {
            break;
        }
    }
    Ok(())
}
