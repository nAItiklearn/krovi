//! What the parser understood. Intents are plain data: nothing here validates a link, an
//! address or a time against the rules; [`crate::plan`] does that and assigns the risk.

use serde::{Deserialize, Serialize};

/// An installed app the host knows about (from the Start menu). `target` is whatever the host
/// needs to launch it; the crate never interprets it, and the host only ever launches targets it
/// listed itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEntry {
    pub name: String,
    pub target: String,
}

/// Which mail app a draft is prepared in. Krovi never sends mail: all three only open a
/// compose window for the user to review and send themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MailProvider {
    #[serde(rename = "gmail")]
    Gmail,
    #[serde(rename = "outlook")]
    Outlook,
    /// A `mailto:` link: opens in whatever the user's default mail app is.
    #[default]
    #[serde(rename = "default")]
    SystemDefault,
}

impl MailProvider {
    pub fn label(self) -> &'static str {
        match self {
            MailProvider::Gmail => "Gmail",
            MailProvider::Outlook => "Outlook",
            MailProvider::SystemDefault => "your default mail app",
        }
    }
}

/// How "open <app>" resolved against the host's app list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum AppMatch {
    Found {
        name: String,
        target: String,
    },
    /// Several apps match equally well: do nothing, ask for the full name.
    Ambiguous {
        names: Vec<String>,
    },
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Intent {
    /// A site from the built-in table (`site` is its key, e.g. "youtube").
    OpenSite {
        site: String,
    },
    /// An address the user typed or said. Not yet validated.
    OpenUrl {
        url: String,
    },
    /// Search a built-in site that has a search page. `defaulted`: no engine was named, Google
    /// was assumed.
    Search {
        site: String,
        query: String,
        defaulted: bool,
    },
    OpenApp {
        query: String,
        app: AppMatch,
    },
    Timer {
        seconds: u64,
        label: Option<String>,
    },
    /// `date` is local `yyyy-mm-dd`, `time` local `HH:MM`; `due_utc_ms` is the same instant at the
    /// UTC offset the parser was given. `default_time`: no time was said, 09:00 was assumed.
    Reminder {
        title: String,
        date: String,
        time: String,
        due_utc_ms: i64,
        default_time: bool,
    },
    Note {
        body: String,
    },
    /// `to` holds every '@' word found (unvalidated); `bad_recipient` is a recipient that was
    /// named but is not an address ("mom"): Krovi does not read contacts.
    Email {
        provider: MailProvider,
        to: Vec<String>,
        subject: String,
        body: String,
        bad_recipient: Option<String>,
    },
    Unknown {
        text: String,
        suggestions: Vec<String>,
    },
}

impl Intent {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Intent::Unknown { .. })
    }
}
