//! Wire types shared with the UI (docs/CONTRACTS.md Â§3). Serialised shapes are the contract:
//! `VoiceCommand` is `{ kind: "open-tool", tool } | { kind: "start-pomodoro" } | ...`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// What the user is doing: dictating a note, or giving a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Note,
    Command,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Note => "note",
            Mode::Command => "command",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "note" => Some(Mode::Note),
            "command" => Some(Mode::Command),
            _ => None,
        }
    }
}

/// Mirror of `ToolId` in `src/lib/types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolId {
    Home,
    Clipboard,
    Paste,
    Timers,
    Pomodoro,
    Shelf,
    Media,
    Calendar,
    Weather,
    Notes,
    Clocks,
    Convert,
    Calculator,
    Screentime,
    /// Quick Transfer (LAN file transfer to/from a phone).
    Transfer,
    /// On-device text summarizer + read-aloud.
    Explain,
    Settings,
}

impl ToolId {
    pub const ALL: [ToolId; 17] = [
        ToolId::Home,
        ToolId::Clipboard,
        ToolId::Paste,
        ToolId::Timers,
        ToolId::Pomodoro,
        ToolId::Shelf,
        ToolId::Media,
        ToolId::Calendar,
        ToolId::Weather,
        ToolId::Notes,
        ToolId::Clocks,
        ToolId::Convert,
        ToolId::Calculator,
        ToolId::Screentime,
        ToolId::Transfer,
        ToolId::Explain,
        ToolId::Settings,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ToolId::Home => "home",
            ToolId::Clipboard => "clipboard",
            ToolId::Paste => "paste",
            ToolId::Timers => "timers",
            ToolId::Pomodoro => "pomodoro",
            ToolId::Shelf => "shelf",
            ToolId::Media => "media",
            ToolId::Calendar => "calendar",
            ToolId::Weather => "weather",
            ToolId::Notes => "notes",
            ToolId::Clocks => "clocks",
            ToolId::Convert => "convert",
            ToolId::Calculator => "calculator",
            ToolId::Screentime => "screentime",
            ToolId::Transfer => "transfer",
            ToolId::Explain => "explain",
            ToolId::Settings => "settings",
        }
    }
}

/// Media transport word. `whats-playing` is a query, the rest are transport commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaCommand {
    #[serde(rename = "playpause")]
    PlayPause,
    #[serde(rename = "play")]
    Play,
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "next")]
    Next,
    #[serde(rename = "previous")]
    Previous,
    #[serde(rename = "whats-playing")]
    WhatsPlaying,
}

impl MediaCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaCommand::PlayPause => "playpause",
            MediaCommand::Play => "play",
            MediaCommand::Pause => "pause",
            MediaCommand::Next => "next",
            MediaCommand::Previous => "previous",
            MediaCommand::WhatsPlaying => "whats-playing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerAction {
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerTarget {
    Timer,
    Pomodoro,
}

/// A parsed voice command (docs/CONTRACTS.md Â§3, extended by Voice v2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum VoiceCommand {
    OpenTool {
        tool: ToolId,
    },
    StartPomodoro,
    StartTimer {
        seconds: u64,
    },
    NewNote {
        body: String,
    },
    Collapse,
    /// A calendar reminder. `date` is `yyyy-mm-dd`, `time` is `HH:MM` (24h) when one was spoken.
    AddReminder {
        date: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        time: Option<String>,
        title: String,
    },
    /// Park the captured selection on the file shelf: file paths, or (for selected text) the text.
    ShelfAdd {
        paths: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// Send the captured selection to the phone through Quick Transfer.
    TransferSend {
        paths: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Media {
        command: MediaCommand,
    },
    TimerControl {
        action: TimerAction,
        target: TimerTarget,
    },
    /// Voice v3: open a folder from the closed set (Known Folders, Quick Access, ...) in
    /// Explorer. `name` is the catalog's display name, `path` the resolved absolute path.
    OpenFolder {
        name: String,
        path: String,
    },
    /// Voice v3: launch an installed Start-menu app. `path` is the launch target
    /// (`shell:AppsFolder\<AUMID>` or a Start-menu `.lnk`).
    OpenApp {
        name: String,
        path: String,
    },
    /// Voice v3: search the Windows Search index by file name.
    SearchFiles {
        query: String,
    },
    /// Voice v3: say what is inside a known folder (count, newest items, size).
    ListFolder {
        name: String,
        path: String,
    },
}

/// What a Voice v3 result row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultKind {
    Folder,
    App,
    File,
}

/// One row of `VoiceResult.results` (search hits, folder contents, the apps an ambiguous name
/// could mean).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultItem {
    pub kind: ResultKind,
    pub name: String,
    /// Absolute path (folder / file) or launch target (app).
    pub path: String,
    /// A short plain-language detail, e.g. "12.4 MB, Sun 1 Nov 10:30 AM".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ResultItem {
    pub fn new(kind: ResultKind, name: impl Into<String>, path: impl Into<String>) -> Self {
        ResultItem {
            kind,
            name: name.into(),
            path: path.into(),
            detail: None,
        }
    }
}

/// What was captured when the session started, exactly as captured ("this" / "it" / "that").
/// `app` is the foreground app's executable name (no window title), when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum VoiceContext {
    /// Selected text in the foreground app.
    Text {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        app: Option<String>,
    },
    /// Selected items in the foreground Explorer window (absolute paths).
    Files {
        value: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        app: Option<String>,
    },
    /// Only the foreground app's name.
    App { value: String },
}

impl VoiceContext {
    pub fn app(&self) -> Option<&str> {
        match self {
            VoiceContext::Text { app, .. } | VoiceContext::Files { app, .. } => app.as_deref(),
            VoiceContext::App { value } => Some(value),
        }
    }
}

/// Final outcome of one listening session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceResult {
    pub mode: Mode,
    /// Everything the recognizer heard, as text ("" when nothing was recognised, and "" for
    /// commands when the user turned off keeping transcripts).
    pub text: String,
    /// `Some` only in command mode, and only when `text` parsed to a known command.
    pub command: Option<VoiceCommand>,
    /// Exactly what context capture read at the start of the session, if anything.
    #[serde(default)]
    pub context: Option<VoiceContext>,
    /// A short plain-language line the UI can show ("Reminder added for Fri 7 Nov, 9:00 AM:
    /// Call mom", "Nothing is selected to send").
    #[serde(default)]
    pub feedback: Option<String>,
    /// `true` when the backend already carried the command out (reminder saved, media key sent):
    /// the UI must only show feedback, not do it again.
    #[serde(default)]
    pub executed: bool,
    /// Voice v3 rows to show in the panel: search hits, a folder's newest items, or the
    /// candidates of an ambiguous name. Empty for every other command.
    #[serde(default)]
    pub results: Vec<ResultItem>,
}

impl VoiceResult {
    pub fn new(mode: Mode, text: impl Into<String>, command: Option<VoiceCommand>) -> Self {
        VoiceResult {
            mode,
            text: text.into(),
            command,
            context: None,
            feedback: None,
            executed: false,
            results: Vec::new(),
        }
    }
}

/// How much Voice may read from the foreground app when a command starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextCapture {
    /// Read nothing, not even the app name.
    #[serde(rename = "off")]
    Off,
    /// App name, selected text (UI Automation) and selected Explorer items.
    #[serde(rename = "selection")]
    Selection,
    /// As above; when UI Automation finds no text, briefly copy with Ctrl+C and restore the clipboard.
    #[serde(rename = "selection+clipboard-fallback")]
    SelectionClipboardFallback,
}

/// The persisted voice privacy settings. `local_only` and `store_audio` are locked: they are
/// displayed, but whatever a caller sends, they come back as `true` / `false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceSettings {
    /// Always `true`: speech is recognised on this PC by Windows SAPI.
    pub local_only: bool,
    /// Always `false`: Krovi never stores audio.
    pub store_audio: bool,
    /// `true` keeps what was said (the dictated note itself; the command text in results).
    /// `false`: voice commands leave no transcript trace anywhere.
    pub keep_transcripts: bool,
    pub context_capture: ContextCapture,
    /// Tidy dictated notes: fillers, stutters, capitalisation, punctuation.
    pub clean_transcription: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            local_only: true,
            store_audio: false,
            keep_transcripts: true,
            context_capture: ContextCapture::Selection,
            clean_transcription: true,
        }
    }
}

impl VoiceSettings {
    /// Re-impose the locked values.
    pub fn normalized(mut self) -> Self {
        self.local_only = true;
        self.store_audio = false;
        self
    }
}

/// Per-session inputs the caller supplies when starting (or transcribing a file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOptions {
    /// Captured at the moment the user triggered voice; used to resolve "this" / "it".
    pub context: Option<VoiceContext>,
    /// Apply the transcript cleaner to dictated notes and spoken note/reminder text.
    pub clean_transcription: bool,
    /// Why there is no usable context, in plain words (capture off, Krovi was in front, ...),
    /// shown instead of the generic "nothing is selected" when a command needed one.
    pub context_hint: Option<String>,
    /// The folders and apps "open ..." / "what's in ..." may name (Voice v3). They become part
    /// of the command grammar when the session starts, and resolve the words afterwards. `None`
    /// disables those commands (only the built-in tools remain).
    pub catalog: Option<std::sync::Arc<crate::catalog::Catalog>>,
}

impl Default for StartOptions {
    fn default() -> Self {
        StartOptions {
            context: None,
            clean_transcription: true,
            context_hint: None,
            catalog: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Listening,
    Processing,
    Error,
}

/// Payload of the `voice-state` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateChange {
    pub state: State,
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StateChange {
    pub fn new(state: State, mode: Mode) -> Self {
        StateChange {
            state,
            mode,
            error: None,
        }
    }

    pub fn error(mode: Mode, message: impl Into<String>) -> Self {
        StateChange {
            state: State::Error,
            mode,
            error: Some(message.into()),
        }
    }
}

/// What the on-device engine can do (`voice_available`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub dictation: bool,
    pub commands: bool,
    pub engine: String,
}

impl Availability {
    pub fn none(reason: impl Into<String>) -> Self {
        Availability {
            dictation: false,
            commands: false,
            engine: reason.into(),
        }
    }
}

/// Receives everything a session produces. Called from the session thread; keep it quick.
pub trait Sink: Send + Sync + 'static {
    fn state(&self, change: StateChange);
    fn partial(&self, text: &str);
    fn result(&self, result: VoiceResult);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceError {
    /// No on-device recognizer, or the crate was built without the `native` feature.
    Unavailable(String),
    NoMicrophone,
    PermissionDenied,
    Engine(String),
    InvalidInput(String),
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceError::Unavailable(m) => write!(f, "Speech recognition is unavailable: {m}"),
            VoiceError::NoMicrophone => f.write_str("No microphone was found. Plug one in or enable it in Windows Sound settings."),
            VoiceError::PermissionDenied => f.write_str("Microphone access is turned off. Enable it under Windows Settings > Privacy & security > Microphone (including \"Let desktop apps access your microphone\")."),
            VoiceError::Engine(m) => write!(f, "Speech engine error: {m}"),
            VoiceError::InvalidInput(m) => write!(f, "Invalid input: {m}"),
        }
    }
}

impl std::error::Error for VoiceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_wire_shapes_match_the_contract() {
        let v = |c: &VoiceCommand| serde_json::to_value(c).unwrap();
        assert_eq!(
            v(&VoiceCommand::OpenTool {
                tool: ToolId::Timers
            }),
            json!({"kind":"open-tool","tool":"timers"})
        );
        assert_eq!(
            v(&VoiceCommand::StartPomodoro),
            json!({"kind":"start-pomodoro"})
        );
        assert_eq!(
            v(&VoiceCommand::StartTimer { seconds: 300 }),
            json!({"kind":"start-timer","seconds":300})
        );
        assert_eq!(
            v(&VoiceCommand::NewNote {
                body: "buy milk".into()
            }),
            json!({"kind":"new-note","body":"buy milk"})
        );
        assert_eq!(v(&VoiceCommand::Collapse), json!({"kind":"collapse"}));
    }

    #[test]
    fn v2_command_wire_shapes() {
        let v = |c: &VoiceCommand| serde_json::to_value(c).unwrap();
        assert_eq!(
            v(&VoiceCommand::AddReminder {
                date: "2026-11-02".into(),
                time: Some("09:00".into()),
                title: "Call mom".into()
            }),
            json!({"kind":"add-reminder","date":"2026-11-02","time":"09:00","title":"Call mom"})
        );
        assert_eq!(
            v(&VoiceCommand::AddReminder {
                date: "2026-11-02".into(),
                time: None,
                title: "Call mom".into()
            }),
            json!({"kind":"add-reminder","date":"2026-11-02","title":"Call mom"}),
            "time is omitted when none was spoken"
        );
        assert_eq!(
            v(&VoiceCommand::ShelfAdd {
                paths: vec!["C:\\a.txt".into()],
                text: None
            }),
            json!({"kind":"shelf-add","paths":["C:\\a.txt"]})
        );
        assert_eq!(
            v(&VoiceCommand::ShelfAdd {
                paths: vec![],
                text: Some("hi".into())
            }),
            json!({"kind":"shelf-add","paths":[],"text":"hi"})
        );
        assert_eq!(
            v(&VoiceCommand::TransferSend {
                paths: vec!["C:\\a.txt".into()],
                text: None
            }),
            json!({"kind":"transfer-send","paths":["C:\\a.txt"]})
        );
        for (c, s) in [
            (MediaCommand::PlayPause, "playpause"),
            (MediaCommand::Play, "play"),
            (MediaCommand::Pause, "pause"),
            (MediaCommand::Next, "next"),
            (MediaCommand::Previous, "previous"),
            (MediaCommand::WhatsPlaying, "whats-playing"),
        ] {
            assert_eq!(
                v(&VoiceCommand::Media { command: c }),
                json!({"kind":"media","command":s})
            );
            assert_eq!(c.as_str(), s);
        }
        assert_eq!(
            v(&VoiceCommand::TimerControl {
                action: TimerAction::Pause,
                target: TimerTarget::Pomodoro
            }),
            json!({"kind":"timer-control","action":"pause","target":"pomodoro"})
        );
    }

    #[test]
    fn context_wire_shapes() {
        let v = |c: &VoiceContext| serde_json::to_value(c).unwrap();
        assert_eq!(
            v(&VoiceContext::Text {
                value: "hello".into(),
                app: Some("notepad".into())
            }),
            json!({"kind":"text","value":"hello","app":"notepad"})
        );
        assert_eq!(
            v(&VoiceContext::Files {
                value: vec!["C:\\x.txt".into()],
                app: Some("explorer".into())
            }),
            json!({"kind":"files","value":["C:\\x.txt"],"app":"explorer"})
        );
        assert_eq!(
            v(&VoiceContext::App {
                value: "chrome".into()
            }),
            json!({"kind":"app","value":"chrome"})
        );
        assert_eq!(
            VoiceContext::Text {
                value: "x".into(),
                app: None
            }
            .app(),
            None
        );
    }

    #[test]
    fn settings_defaults_locks_and_wire_shape() {
        let d = VoiceSettings::default();
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            json!({"localOnly":true,"storeAudio":false,"keepTranscripts":true,"contextCapture":"selection","cleanTranscription":true})
        );
        // locked fields cannot be changed by a caller
        let hostile: VoiceSettings = serde_json::from_value(
            json!({"localOnly":false,"storeAudio":true,"keepTranscripts":false,"contextCapture":"selection+clipboard-fallback","cleanTranscription":false}),
        )
        .unwrap();
        let n = hostile.normalized();
        assert!(n.local_only && !n.store_audio);
        assert!(!n.keep_transcripts && !n.clean_transcription);
        assert_eq!(
            n.context_capture,
            ContextCapture::SelectionClipboardFallback
        );
        // a partial object (older client) fills the rest from defaults
        let partial: VoiceSettings =
            serde_json::from_value(json!({"contextCapture":"off"})).unwrap();
        assert_eq!(partial.context_capture, ContextCapture::Off);
        assert!(partial.keep_transcripts && partial.clean_transcription);
        assert!(
            serde_json::from_value::<VoiceSettings>(json!({"contextCapture":"everything"}))
                .is_err()
        );
    }

    #[test]
    fn result_and_state_wire_shapes() {
        let r = VoiceResult::new(Mode::Command, "close", Some(VoiceCommand::Collapse));
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"mode":"command","text":"close","command":{"kind":"collapse"},"context":null,"feedback":null,"executed":false,"results":[]})
        );
        let r = VoiceResult::new(Mode::Note, "hi", None);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"mode":"note","text":"hi","command":null,"context":null,"feedback":null,"executed":false,"results":[]})
        );
        let mut r = VoiceResult::new(Mode::Command, "next track", None);
        r.context = Some(VoiceContext::App {
            value: "spotify".into(),
        });
        r.feedback = Some("Skipped".into());
        r.executed = true;
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"mode":"command","text":"next track","command":null,"context":{"kind":"app","value":"spotify"},"feedback":"Skipped","executed":true,"results":[]})
        );
        // Voice v3 shapes
        let mut r = VoiceResult::new(
            Mode::Command,
            "find invoice",
            Some(VoiceCommand::SearchFiles {
                query: "invoice".into(),
            }),
        );
        r.results = vec![
            ResultItem::new(ResultKind::File, "invoice.pdf", r"C:\Docs\invoice.pdf"),
            ResultItem {
                detail: Some("1.0 MB, Sun 1 Nov 10:30 AM".into()),
                ..ResultItem::new(ResultKind::Folder, "Invoices", r"C:\Docs\Invoices")
            },
        ];
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"mode":"command","text":"find invoice","command":{"kind":"search-files","query":"invoice"},"context":null,"feedback":null,"executed":false,"results":[{"kind":"file","name":"invoice.pdf","path":"C:\\Docs\\invoice.pdf"},{"kind":"folder","name":"Invoices","path":"C:\\Docs\\Invoices","detail":"1.0 MB, Sun 1 Nov 10:30 AM"}]})
        );
        assert_eq!(
            serde_json::to_value(VoiceCommand::OpenFolder {
                name: "Downloads".into(),
                path: r"C:\u\Downloads".into()
            })
            .unwrap(),
            json!({"kind":"open-folder","name":"Downloads","path":"C:\\u\\Downloads"})
        );
        assert_eq!(
            serde_json::to_value(VoiceCommand::OpenApp {
                name: "Notepad".into(),
                path: "shell:AppsFolder\\X".into()
            })
            .unwrap(),
            json!({"kind":"open-app","name":"Notepad","path":"shell:AppsFolder\\X"})
        );
        assert_eq!(
            serde_json::to_value(VoiceCommand::ListFolder {
                name: "Docs".into(),
                path: "D:\\Docs".into()
            })
            .unwrap(),
            json!({"kind":"list-folder","name":"Docs","path":"D:\\Docs"})
        );
        // an older payload without `results` still deserialises
        let old: VoiceResult = serde_json::from_value(
            json!({"mode":"note","text":"hi","command":null,"context":null,"feedback":null,"executed":false}),
        )
        .unwrap();
        assert!(old.results.is_empty());
        let s = StateChange::new(State::Listening, Mode::Note);
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!({"state":"listening","mode":"note"})
        );
        let s = StateChange::error(Mode::Note, "boom");
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!({"state":"error","mode":"note","error":"boom"})
        );
    }

    #[test]
    fn tool_ids_cover_every_frontend_tool() {
        // Must stay in lock-step with ToolId in src/lib/types.ts.
        let names: Vec<&str> = ToolId::ALL.iter().map(|t| t.as_str()).collect();
        assert_eq!(
            names,
            [
                "home",
                "clipboard",
                "paste",
                "timers",
                "pomodoro",
                "shelf",
                "media",
                "calendar",
                "weather",
                "notes",
                "clocks",
                "convert",
                "calculator",
                "screentime",
                "transfer",
                "explain",
                "settings"
            ]
        );
        for t in ToolId::ALL {
            assert_eq!(serde_json::to_value(t).unwrap(), json!(t.as_str()));
        }
    }

    #[test]
    fn mode_parses() {
        assert_eq!(Mode::parse("note"), Some(Mode::Note));
        assert_eq!(Mode::parse(" Command "), Some(Mode::Command));
        assert_eq!(Mode::parse("x"), None);
    }
}
