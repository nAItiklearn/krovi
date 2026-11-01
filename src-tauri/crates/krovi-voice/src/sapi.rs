//! Windows SAPI 5 engine (the in-process "desktop" recognizer, fully on-device).
//!
//! Threading: every COM object lives on ONE dedicated thread per session, initialised as an MTA
//! (`COINIT_MULTITHREADED`). Notifications use a Win32 event handle (`SetNotifyWin32Event`), so
//! no window or message pump is needed and nothing can deadlock against the UI thread, which is
//! a Tauri/WebView2 STA. The same session loop drives microphone and WAV-file input; only the
//! input binding and the end-of-session rules (see `session::Policy`) differ.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{w, Interface, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, HANDLE, RPC_E_CHANGED_MODE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::waveInGetNumDevs;
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::WaitForSingleObject;

use crate::catalog::Catalog;
use crate::civil::LocalNow;
use crate::grammar::{command_templates, Slot, Template, ONES, TEENS, TENS};
use crate::result::build_result;
use crate::session::{Collector, Finish, Policy, Raw};
use crate::types::{
    Availability, Mode, Sink, StartOptions, State, StateChange, VoiceError, VoiceResult,
};

/// `SP_GETWHOLEPHRASE` / `SPPR_ALL_ELEMENTS`.
const WHOLE_PHRASE: u32 = u32::MAX;
const RULE_COMMANDS: u32 = 1;
const GRAMMAR_DICTATION: u64 = 1;
const GRAMMAR_COMMANDS: u64 = 2;

/// SAPI's `CFGConfidenceRejectionThreshold` (0-100, engine default 60) for the command grammar.
/// Measured on synthesised speech (see docs/DECISIONS.md D-012): 60 recognised 84% of commands
/// with no false accepts; 40 recognised 97% with false accepts only on deliberately command-like
/// chatter; 30 and below let in far too much noise. 40 is the knee.
const COMMAND_REJECTION_THRESHOLD: i32 = 40;

/// Dictation results the engine itself scores below this are noise, not speech. Measured: real
/// dictation scores 0.55-0.75 on clean speech; a lone hallucinated word in room noise scored 0.09.
const MIN_DICTATION_CONFIDENCE: f32 = 0.15;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// `KROVI_VOICE_TRACE=1` prints recognizer diagnostics to stderr (local only, never sent anywhere).
fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("KROVI_VOICE_TRACE").is_some())
}

fn engine_err(context: &str, e: windows::core::Error) -> VoiceError {
    VoiceError::Engine(format!(
        "{context}: {} (0x{:08X})",
        e.message().trim(),
        e.code().0 as u32
    ))
}

/// `SPFEI(...)`: an event-interest mask must carry the two reserved check bits.
fn event_mask(ids: &[SPEVENTENUM]) -> u64 {
    let check = (1u64 << SPEI_RESERVED1.0) | (1u64 << SPEI_RESERVED2.0);
    ids.iter().fold(check, |m, id| m | (1u64 << id.0))
}

struct ComGuard;

impl ComGuard {
    fn init() -> Result<ComGuard, VoiceError> {
        // SAFETY: plain COM init on a thread we own; balanced by Drop.
        let hr: HRESULT = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Ok(ComGuard)
        } else if hr == RPC_E_CHANGED_MODE {
            Err(VoiceError::Engine(
                "COM was already initialised as single-threaded on the speech thread".into(),
            ))
        } else {
            Err(VoiceError::Engine(format!(
                "CoInitializeEx failed (0x{:08X})",
                hr.0 as u32
            )))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: matches the successful CoInitializeEx in `init`.
        unsafe { CoUninitialize() };
    }
}

// ---------------------------------------------------------------------------------------------
// microphone preflight (clear errors instead of silent failure)
// ---------------------------------------------------------------------------------------------

fn reg_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let sub = wide(subkey);
    let val = wide(value);
    let mut buf = [0u16; 64];
    let mut len = std::mem::size_of_val(&buf) as u32;
    // SAFETY: buffers are valid for `len` bytes; the call only writes within them.
    let rc = unsafe {
        RegGetValueW(
            root,
            PCWSTR(sub.as_ptr()),
            PCWSTR(val.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut len),
        )
    };
    if !rc.is_ok() {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1).min(buf.len());
    String::from_utf16(&buf[..chars]).ok()
}

/// True when Windows' microphone privacy switches block desktop apps like Krovi.
fn mic_consent_denied() -> bool {
    const BASE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let nonpackaged = format!(r"{BASE}\NonPackaged");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .any(|root| {
            [BASE, nonpackaged.as_str()].into_iter().any(|key| {
                reg_string(root, key, "Value").is_some_and(|v| v.eq_ignore_ascii_case("deny"))
            })
        })
}

fn preflight_mic() -> Result<(), VoiceError> {
    // SAFETY: no arguments; reads the device count.
    if unsafe { waveInGetNumDevs() } == 0 {
        return Err(VoiceError::NoMicrophone);
    }
    if mic_consent_denied() {
        return Err(VoiceError::PermissionDenied);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// the recognizer
// ---------------------------------------------------------------------------------------------

pub(crate) enum Input<'a> {
    Mic,
    File(&'a Path),
}

/// Field order is drop order: grammars, then context, then recognizer, then the input.
struct Engine {
    dictation: Option<ISpRecoGrammar>,
    commands: Option<ISpRecoGrammar>,
    context: ISpRecoContext,
    recognizer: ISpRecognizer,
    event: HANDLE,
    /// The audio source (microphone or file stream); must outlive the recognizer's use of it.
    _input: windows::core::IUnknown,
}

impl Engine {
    /// Build the recognizer and its grammars. Does not start listening yet.
    ///
    /// SAFETY: must be called on a thread that has initialised COM (an MTA).
    unsafe fn create(input: &Input, mode: Mode, catalog: &Catalog) -> Result<Engine, VoiceError> {
        let recognizer: ISpRecognizer = CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL)
            .map_err(|e| engine_err("no on-device speech recognizer", e))?;
        // Stay silent (and keep the microphone closed) until `activate`.
        recognizer
            .SetRecoState(SPRST_INACTIVE)
            .map_err(|e| engine_err("could not park the recognizer", e))?;

        // Privacy: do not let the recognizer learn from, and persist into the user's Windows speech
        // profile, what is dictated into Krovi (deleting a note should leave no trace behind).
        // Best effort; "AdaptationOn" is the SAPI desktop engine's documented switch.
        let _ = recognizer.SetPropertyNum(w!("AdaptationOn"), 0);
        if tracing() {
            let mut now = -1i32;
            let r = recognizer.GetPropertyNum(w!("AdaptationOn"), &mut now);
            eprintln!("[voice] AdaptationOn={now} ({r:?})");
        }
        if mode == Mode::Command {
            // Best effort: a failure just leaves the engine's own default (60) in force.
            let _ = recognizer.SetPropertyNum(
                w!("CFGConfidenceRejectionThreshold"),
                COMMAND_REJECTION_THRESHOLD,
            );
        }

        let input_keepalive: windows::core::IUnknown = match input {
            Input::File(path) => {
                let stream: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL)
                    .map_err(|e| engine_err("could not create an audio stream", e))?;
                let name = wide_path(path);
                stream
                    .BindToFile(PCWSTR(name.as_ptr()), SPFM_OPEN_READONLY, None, None, 0)
                    .map_err(|e| engine_err("could not open the audio file", e))?;
                recognizer
                    .SetInput(&stream, BOOL(1))
                    .map_err(|e| engine_err("could not attach the audio file", e))?;
                stream.cast().map_err(|e| engine_err("audio stream", e))?
            }
            Input::Mic => {
                // Bind the Windows default capture device explicitly (WAVE_MAPPER = -1) instead of
                // relying on SAPI's implicit default, so a missing/blocked device fails here.
                let audio: ISpMMSysAudio = CoCreateInstance(&SpMMAudioIn, None, CLSCTX_ALL)
                    .map_err(|e| engine_err("no audio input available", e))?;
                audio
                    .SetDeviceId(u32::MAX)
                    .map_err(|e| engine_err("could not select the default microphone", e))?;
                recognizer
                    .SetInput(&audio, BOOL(1))
                    .map_err(|e| engine_err("could not attach the microphone", e))?;
                audio.cast().map_err(|e| engine_err("microphone", e))?
            }
        };

        let context = recognizer
            .CreateRecoContext()
            .map_err(|e| engine_err("could not create a recognition context", e))?;
        context
            .SetNotifyWin32Event()
            .map_err(|e| engine_err("could not register for recognizer events", e))?;
        let mut events = vec![
            SPEI_RECOGNITION,
            SPEI_HYPOTHESIS,
            SPEI_SOUND_START,
            SPEI_SOUND_END,
            SPEI_PHRASE_START,
            SPEI_FALSE_RECOGNITION,
            SPEI_END_SR_STREAM,
        ];
        if tracing() {
            events.push(SPEI_SR_AUDIO_LEVEL); // diagnostics only: shows whether the microphone delivers signal
        }
        let interest = event_mask(&events);
        context
            .SetInterest(interest, interest)
            .map_err(|e| engine_err("could not select recognizer events", e))?;
        let event = context.GetNotifyEventHandle();

        // Notes use free dictation. Commands use ONLY the closed grammar (with dictation embedded
        // for the body of "new note ..."): measured against running both together, the grammar-first
        // strategy was far more accurate on short commands (docs/DECISIONS.md D-012).
        let dictation = if mode == Mode::Note {
            let g = context
                .CreateGrammar(GRAMMAR_DICTATION)
                .map_err(|e| engine_err("could not create the dictation grammar", e))?;
            g.LoadDictation(PCWSTR::null(), SPLO_STATIC)
                .map_err(|e| engine_err("dictation is not available for this recognizer", e))?;
            Some(g)
        } else {
            None
        };
        let commands = if mode == Mode::Command {
            let g = context
                .CreateGrammar(GRAMMAR_COMMANDS)
                .map_err(|e| engine_err("could not create the command grammar", e))?;
            match build_command_grammar(&g, catalog) {
                Ok(()) => Some(g),
                Err(e) if !catalog.is_empty() => {
                    // A folder or app name the engine cannot take must never cost the user the
                    // built-in commands: rebuild without the dynamic names.
                    if tracing() {
                        eprintln!("[voice] grammar with {} folders / {} apps failed ({e}); using built-ins only", catalog.folders.len(), catalog.apps.len());
                    }
                    drop(g);
                    let g = context
                        .CreateGrammar(GRAMMAR_COMMANDS)
                        .map_err(|e| engine_err("could not create the command grammar", e))?;
                    build_command_grammar(&g, &Catalog::default())
                        .map_err(|e| engine_err("could not build the command grammar", e))?;
                    Some(g)
                }
                Err(e) => return Err(engine_err("could not build the command grammar", e)),
            }
        } else {
            None
        };

        Ok(Engine {
            dictation,
            commands,
            context,
            recognizer,
            event,
            _input: input_keepalive,
        })
    }

    /// Start recognising (opens the microphone for live input).
    unsafe fn activate(&self) -> Result<(), VoiceError> {
        if let Some(g) = &self.dictation {
            g.SetDictationState(SPRS_ACTIVE)
                .map_err(|e| engine_err("could not enable dictation", e))?;
        }
        if let Some(g) = &self.commands {
            g.SetRuleIdState(RULE_COMMANDS, SPRS_ACTIVE)
                .map_err(|e| engine_err("could not enable the command grammar", e))?;
        }
        self.recognizer.SetRecoState(SPRST_ACTIVE).map_err(|e| {
            engine_err(
                "could not start listening (is another app using the microphone?)",
                e,
            )
        })
    }

    fn deactivate(&self) {
        // SAFETY: state change on a live recognizer; failure is harmless at shutdown.
        unsafe {
            let _ = self.recognizer.SetRecoState(SPRST_INACTIVE);
        }
    }

    /// Wait up to `ms` for recognizer activity.
    fn wait(&self, ms: u32) {
        // SAFETY: `event` is owned by the context, which outlives this call.
        unsafe {
            let r = WaitForSingleObject(self.event, ms);
            let _ = r == WAIT_OBJECT_0;
        }
    }

    /// Pull every queued event and decode it.
    unsafe fn drain(&self, out: &mut Vec<Raw>) {
        loop {
            let mut batch = [SPEVENT::default(); 8];
            let mut fetched = 0u32;
            if self
                .context
                .GetEvents(batch.len() as u32, batch.as_mut_ptr(), &mut fetched)
                .is_err()
                || fetched == 0
            {
                return;
            }
            for e in &batch[..fetched as usize] {
                decode(e, out);
            }
            if (fetched as usize) < batch.len() {
                return;
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.deactivate();
    }
}

/// Decode one SPEVENT, taking ownership of (and releasing) whatever `lParam` points at.
unsafe fn decode(e: &SPEVENT, out: &mut Vec<Raw>) {
    let id = e._bitfield & 0xFFFF;
    let param_type = (e._bitfield >> 16) & 0xFFFF;
    let lparam = e.lParam.0;

    // The event owns one reference / allocation; free it no matter what we do with the event.
    let object: Option<windows::core::IUnknown> =
        if param_type == SPET_LPARAM_IS_OBJECT.0 || param_type == SPET_LPARAM_IS_TOKEN.0 {
            if lparam != 0 {
                Some(windows::core::IUnknown::from_raw(lparam as *mut c_void))
            } else {
                None
            }
        } else {
            if (param_type == SPET_LPARAM_IS_STRING.0 || param_type == SPET_LPARAM_IS_POINTER.0)
                && lparam != 0
            {
                CoTaskMemFree(Some(lparam as *const c_void));
            }
            None
        };

    let text_of = |obj: &Option<windows::core::IUnknown>| -> Option<String> {
        obj.as_ref()
            .and_then(|o| o.cast::<ISpRecoResult>().ok())
            .and_then(|r| result_text(&r))
    };

    match SPEVENTENUM(id) {
        SPEI_SOUND_START => out.push(Raw::SoundStart),
        SPEI_SOUND_END => out.push(Raw::SoundEnd),
        SPEI_PHRASE_START => out.push(Raw::PhraseStart),
        SPEI_HYPOTHESIS => {
            if let Some(t) = text_of(&object) {
                out.push(Raw::Hypothesis(t));
            }
        }
        SPEI_RECOGNITION => {
            let result = object.as_ref().and_then(|o| o.cast::<ISpRecoResult>().ok());
            let info = result.as_ref().and_then(phrase_info);
            let accepted = info.is_none_or(|i| {
                i.grammar == GRAMMAR_COMMANDS
                    || (i.grammar == GRAMMAR_DICTATION
                        && i.engine_confidence >= MIN_DICTATION_CONFIDENCE)
            });
            let text = result
                .as_ref()
                .and_then(result_text)
                .filter(|t| !t.trim().is_empty());
            if tracing() {
                eprintln!(
                    "[voice] recognition grammar={:?} text={:?} accepted={accepted}",
                    info, text
                );
            }
            match text {
                Some(t) if accepted => {
                    if let Some(i) = info.filter(|i| i.grammar == GRAMMAR_COMMANDS) {
                        let pct = (i.engine_confidence * 100.0).clamp(0.0, 100.0).round() as u8;
                        out.push(Raw::Confidence(pct));
                    }
                    out.push(Raw::Final(t));
                }
                _ => out.push(Raw::Rejected),
            }
        }
        SPEI_FALSE_RECOGNITION => out.push(Raw::Rejected),
        SPEI_END_SR_STREAM => out.push(Raw::StreamEnd),
        SPEI_SR_AUDIO_LEVEL if tracing() => eprintln!("[voice] audio level {}", e.wParam.0),
        _ => {}
    }
}

#[derive(Debug, Clone, Copy)]
struct PhraseInfo {
    grammar: u64,
    engine_confidence: f32,
}

fn phrase_info(r: &ISpRecoResult) -> Option<PhraseInfo> {
    // SAFETY: GetPhrase returns a CoTaskMem block we own; it is read once and freed.
    unsafe {
        let p = r.GetPhrase().ok()?;
        if p.is_null() {
            return None;
        }
        let info = PhraseInfo {
            grammar: (*p).Base.ullGrammarID,
            engine_confidence: (*p).Base.Rule.SREngineConfidence,
        };
        CoTaskMemFree(Some(p as *const c_void));
        Some(info)
    }
}

fn result_text(r: &ISpRecoResult) -> Option<String> {
    let mut p = PWSTR::null();
    // SAFETY: GetText allocates the string with CoTaskMemAlloc; we free it below.
    unsafe {
        r.GetText(WHOLE_PHRASE, WHOLE_PHRASE, BOOL(1), &mut p, None)
            .ok()?;
        if p.is_null() {
            return None;
        }
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

// ---------------------------------------------------------------------------------------------
// the command grammar (built dynamically from grammar::command_templates)
// ---------------------------------------------------------------------------------------------

const NO_STATE: SPSTATEHANDLE = SPSTATEHANDLE(null_mut());

unsafe fn new_state(
    g: &ISpRecoGrammar,
    from: SPSTATEHANDLE,
) -> windows::core::Result<SPSTATEHANDLE> {
    let mut h = NO_STATE;
    g.CreateNewState(from, &mut h)?;
    Ok(h)
}

unsafe fn add_word(
    g: &ISpRecoGrammar,
    from: SPSTATEHANDLE,
    to: SPSTATEHANDLE,
    word: &str,
) -> windows::core::Result<()> {
    let wide = wide(word);
    g.AddWordTransition(
        from,
        to,
        PCWSTR(wide.as_ptr()),
        w!(" "),
        SPWT_LEXICAL,
        1.0,
        null(),
    )
}

unsafe fn add_epsilon(
    g: &ISpRecoGrammar,
    from: SPSTATEHANDLE,
    to: SPSTATEHANDLE,
) -> windows::core::Result<()> {
    g.AddWordTransition(
        from,
        to,
        PCWSTR::null(),
        PCWSTR::null(),
        SPWT_LEXICAL,
        1.0,
        null(),
    )
}

/// `from --w1--> .. --wn--> to` for a possibly multi-word alternative ("go to").
unsafe fn add_phrase(
    g: &ISpRecoGrammar,
    from: SPSTATEHANDLE,
    to: SPSTATEHANDLE,
    phrase: &str,
) -> windows::core::Result<()> {
    let words: Vec<&str> = phrase.split_whitespace().collect();
    let mut cur = from;
    for (i, word) in words.iter().enumerate() {
        let next = if i + 1 == words.len() {
            to
        } else {
            new_state(g, cur)?
        };
        add_word(g, cur, next, word)?;
        cur = next;
    }
    Ok(())
}

unsafe fn build_numbers(g: &ISpRecoGrammar, num: SPSTATEHANDLE) -> windows::core::Result<()> {
    for w in ONES.iter().chain(TEENS) {
        add_word(g, num, NO_STATE, w)?;
    }
    let units = new_state(g, num)?;
    for t in TENS {
        add_word(g, num, units, t)?;
    }
    add_epsilon(g, units, NO_STATE)?; // "twenty"
    for w in ONES {
        add_word(g, units, NO_STATE, w)?; // "twenty five"
    }
    Ok(())
}

unsafe fn add_template(
    g: &ISpRecoGrammar,
    top: SPSTATEHANDLE,
    num: SPSTATEHANDLE,
    template: &Template,
) -> windows::core::Result<()> {
    let mut cur = top;
    for (i, slot) in template.iter().enumerate() {
        let next = if i + 1 == template.len() {
            NO_STATE
        } else {
            new_state(g, cur)?
        };
        match slot {
            Slot::Any(alts) => {
                for a in alts {
                    add_phrase(g, cur, next, a)?;
                }
            }
            Slot::Opt(alts) => {
                for a in alts {
                    add_phrase(g, cur, next, a)?;
                }
                add_epsilon(g, cur, next)?;
            }
            Slot::Num => g.AddRuleTransition(cur, next, num, 1.0, null())?,
            // SPINFDICTATION: one or more words of free dictation.
            Slot::Dictation => add_word(g, cur, next, "*+")?,
        }
        cur = next;
    }
    Ok(())
}

unsafe fn build_command_grammar(
    g: &ISpRecoGrammar,
    catalog: &Catalog,
) -> windows::core::Result<()> {
    let mut top = NO_STATE;
    g.GetRule(
        w!("commands"),
        RULE_COMMANDS,
        (SPRAF_TopLevel.0 | SPRAF_Active.0 | SPRAF_Dynamic.0) as u32,
        BOOL(1),
        &mut top,
    )?;
    let mut num = NO_STATE;
    g.GetRule(w!("number"), 2, SPRAF_Dynamic.0 as u32, BOOL(1), &mut num)?;
    build_numbers(g, num)?;
    for template in command_templates(catalog) {
        add_template(g, top, num, &template)?;
    }
    g.Commit(0)
}

// ---------------------------------------------------------------------------------------------
// availability
// ---------------------------------------------------------------------------------------------

/// Probe without touching the microphone: create the recognizer, load dictation, build the
/// grammar. Runs on its own MTA thread; the result is cached by the caller.
pub(crate) fn probe() -> Availability {
    let run = || -> Result<Availability, VoiceError> {
        let _com = ComGuard::init()?;
        // SAFETY: COM is initialised on this thread; objects are dropped before `_com`.
        unsafe {
            let recognizer: ISpRecognizer = CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL)
                .map_err(|e| engine_err("no on-device speech recognizer", e))?;
            recognizer
                .SetRecoState(SPRST_INACTIVE)
                .map_err(|e| engine_err("recognizer", e))?;
            let name = recognizer
                .GetRecognizer()
                .ok()
                .and_then(|t| t.GetStringValue(PCWSTR::null()).ok())
                .and_then(|p| {
                    let s = p.to_string().ok();
                    CoTaskMemFree(Some(p.0 as *const c_void));
                    s
                })
                .unwrap_or_else(|| "unknown recognizer".to_string());
            let context = recognizer
                .CreateRecoContext()
                .map_err(|e| engine_err("recognition context", e))?;
            let dictation = context
                .CreateGrammar(GRAMMAR_DICTATION)
                .and_then(|g| g.LoadDictation(PCWSTR::null(), SPLO_STATIC))
                .is_ok();
            let commands = context
                .CreateGrammar(GRAMMAR_COMMANDS)
                .and_then(|g| build_command_grammar(&g, &Catalog::default()))
                .is_ok();
            Ok(Availability {
                dictation,
                commands,
                engine: format!("Windows SAPI 5 (on-device): {name}"),
            })
        }
    };
    thread::Builder::new()
        .name("krovi-voice-probe".into())
        .spawn(move || run().unwrap_or_else(|e| Availability::none(e.to_string())))
        .ok()
        .and_then(|h| h.join().ok())
        .unwrap_or_else(|| Availability::none("speech probe failed"))
}

// ---------------------------------------------------------------------------------------------
// sessions
// ---------------------------------------------------------------------------------------------

fn finish_result(mode: Mode, collector: &Collector, opts: &StartOptions) -> VoiceResult {
    build_result(
        mode,
        &collector.final_parts(),
        opts,
        LocalNow::system(),
        collector.min_confidence(),
    )
}

/// Run the listen loop until the policy says stop. `stop` is `None` for file input.
unsafe fn drive(
    engine: &Engine,
    policy: Policy,
    stop: Option<&AtomicBool>,
    sink: Option<&dyn Sink>,
) -> (Collector, Finish) {
    let t0 = Instant::now();
    let mut collector = Collector::new(policy);
    let mut raws: Vec<Raw> = Vec::new();
    let mut last_partial = String::new();
    let mut last_event = Instant::now();
    loop {
        engine.wait(50);
        raws.clear();
        engine.drain(&mut raws);
        let now = t0.elapsed().as_millis() as u64;
        if !raws.is_empty() {
            last_event = Instant::now();
        }
        for raw in raws.drain(..) {
            if let Some(partial) = collector.on_raw(raw, now) {
                if partial != last_partial {
                    if let Some(s) = sink {
                        s.partial(&partial);
                    }
                    last_partial = partial;
                }
            }
        }
        if stop.is_some_and(|s| s.load(Ordering::SeqCst)) {
            collector.request_stop(now);
        }
        if let Some(reason) = collector.poll(now) {
            return (collector, reason);
        }
        // File input that stalls without ever signalling its end must not hang a caller.
        if stop.is_none() && last_event.elapsed() > Duration::from_secs(10) {
            return (collector, Finish::StreamEnd);
        }
    }
}

/// Handle to a running microphone session.
pub(crate) struct MicSession {
    pub(crate) mode: Mode,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl MicSession {
    pub(crate) fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Block until the session thread has finished (or `timeout` passes).
    pub(crate) fn wait_done(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.is_done() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if self.is_done() {
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
            true
        } else {
            false
        }
    }
}

pub(crate) fn start_mic(
    mode: Mode,
    sink: Arc<dyn Sink>,
    opts: StartOptions,
) -> Result<MicSession, VoiceError> {
    if let Err(e) = preflight_mic() {
        sink.state(StateChange::error(mode, e.to_string()));
        return Err(e);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), VoiceError>>();
    /// Marks the session finished even if the thread panics, so `Voice` can never wedge.
    struct DoneOnDrop(Arc<AtomicBool>);
    impl Drop for DoneOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let join = {
        let (stop, done, sink) = (stop.clone(), done.clone(), sink.clone());
        thread::Builder::new()
            .name("krovi-voice".into())
            .spawn(move || {
                let _done = DoneOnDrop(done);
                run_mic(mode, sink, stop, ready_tx, opts);
            })
            .map_err(|e| VoiceError::Engine(format!("could not start the speech thread: {e}")))?
    };
    let failure = match ready_rx.recv_timeout(Duration::from_secs(12)) {
        Ok(Ok(())) => {
            return Ok(MicSession {
                mode,
                stop,
                done,
                join: Some(join),
            })
        }
        Ok(Err(e)) => {
            let _ = join.join();
            e
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            stop.store(true, Ordering::SeqCst); // if it ever wakes up it must not start listening
            VoiceError::Engine("the speech engine did not start in time".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = join.join();
            VoiceError::Engine("the speech engine stopped unexpectedly while starting".into())
        }
    };
    sink.state(StateChange::error(mode, failure.to_string()));
    Err(failure)
}

fn run_mic(
    mode: Mode,
    sink: Arc<dyn Sink>,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<Result<(), VoiceError>>,
    opts: StartOptions,
) {
    let _com = match ComGuard::init() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    // SAFETY: COM initialised on this thread; the engine is dropped before `_com`.
    unsafe {
        let empty = Catalog::default();
        let catalog = opts.catalog.as_deref().unwrap_or(&empty);
        let engine = match Engine::create(&Input::Mic, mode, catalog) {
            Ok(e) => e,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        if stop.load(Ordering::SeqCst) {
            return; // the caller gave up waiting; do not open the microphone late
        }
        if let Err(e) = engine.activate() {
            let _ = ready.send(Err(e));
            return;
        }
        let _ = ready.send(Ok(()));
        sink.state(StateChange::new(State::Listening, mode));

        let (collector, finish) = drive(&engine, Policy::for_mode(mode), Some(&stop), Some(&*sink));
        engine.deactivate();

        sink.state(StateChange::new(State::Processing, mode));
        if finish == Finish::StreamEnd && collector.final_text().is_empty() {
            sink.state(StateChange::error(
                mode,
                "The microphone stopped delivering audio.",
            ));
            return;
        }
        sink.result(finish_result(mode, &collector, &opts));
        sink.state(StateChange::new(State::Idle, mode));
    }
}

/// Feed a WAV file through exactly the same recognizer, grammars and parser as the microphone.
pub(crate) fn transcribe_file(
    path: &Path,
    mode: Mode,
    opts: StartOptions,
) -> Result<VoiceResult, VoiceError> {
    if !path.is_file() {
        return Err(VoiceError::InvalidInput(format!(
            "audio file not found: {}",
            path.display()
        )));
    }
    let path = path.to_path_buf();
    thread::Builder::new()
        .name("krovi-voice-file".into())
        .spawn(move || -> Result<VoiceResult, VoiceError> {
            let _com = ComGuard::init()?;
            // SAFETY: COM initialised on this thread; the engine is dropped before `_com`.
            unsafe {
                let empty = Catalog::default();
                let catalog = opts.catalog.as_deref().unwrap_or(&empty);
                let engine = Engine::create(&Input::File(&path), mode, catalog)?;
                engine.activate()?;
                let (collector, _) = drive(&engine, Policy::for_file(), None, None);
                engine.deactivate();
                Ok(finish_result(mode, &collector, &opts))
            }
        })
        .map_err(|e| VoiceError::Engine(format!("could not start the speech thread: {e}")))?
        .join()
        .map_err(|_| VoiceError::Engine("the speech thread panicked".into()))?
}
