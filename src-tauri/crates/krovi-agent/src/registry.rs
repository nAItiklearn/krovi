//! The integration catalog and the MCP server registry. Both are *honest*:
//!
//! * The catalog lists what Agent Mode can actually do today (`Builtin`), what is not available
//!   yet (`ComingSoon`), and what the user registered but nothing connects to (`NotConnected`),
//!   each with the data it touches and who makes the network request.
//! * The MCP registry is **registry only**. It stores the servers a user typed in, validated,
//!   with `enabled: false` by default. Nothing in Krovi starts an MCP server, connects to one,
//!   or calls a tool on one; the UI says so.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::url;

/// Settings key the host persists the registry under (a JSON array of [`McpServer`]).
pub const MCP_SETTINGS_KEY: &str = "agent.mcp.v1";
pub const MAX_MCP_SERVERS: usize = 20;
pub const MAX_MCP_NAME_CHARS: usize = 40;
pub const MAX_MCP_ARGS: usize = 32;
pub const MAX_MCP_ARG_CHARS: usize = 500;
pub const MAX_MCP_COMMAND_CHARS: usize = 260;

// ---------------------------------------------------------------------------------------------
// the catalog
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationStatus {
    /// Works today, inside Krovi.
    Builtin,
    /// Registered by the user, but nothing is connected to it.
    NotConnected,
    /// Not available yet.
    ComingSoon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub status: IntegrationStatus,
    /// What data it touches.
    pub data: String,
    /// Whether anything leaves this PC, and who sends it.
    pub network: String,
}

struct Row {
    id: &'static str,
    name: &'static str,
    summary: &'static str,
    status: IntegrationStatus,
    data: &'static str,
    network: &'static str,
}

const BROWSER_ONLY: &str =
    "Your browser makes the request when the page opens. Krovi sends nothing itself.";
const LOCAL_ONLY: &str = "None. This stays on this PC.";

const CATALOG: &[Row] = &[
    Row {
        id: "websites",
        name: "Websites",
        summary: "Opens well-known sites (YouTube, GitHub, Wikipedia and more) in your browser. An address you type yourself asks for your OK first.",
        status: IntegrationStatus::Builtin,
        data: "The site you named. Nothing else.",
        network: BROWSER_ONLY,
    },
    Row {
        id: "google-search",
        name: "Google search",
        summary: "Opens a Google results page for the words you give.",
        status: IntegrationStatus::Builtin,
        data: "The words you searched for, inside the link your browser opens.",
        network: "Your browser sends the search to Google. Krovi sends nothing itself.",
    },
    Row {
        id: "youtube",
        name: "YouTube",
        summary: "Opens YouTube or its search results. It does not play anything by itself.",
        status: IntegrationStatus::Builtin,
        data: "The words you searched for, inside the link your browser opens.",
        network: "Your browser sends the search to YouTube. Krovi sends nothing itself.",
    },
    Row {
        id: "apps",
        name: "Apps",
        summary: "Launches apps from your Start menu.",
        status: IntegrationStatus::Builtin,
        data: "The names of your Start-menu apps, read on this PC.",
        network: LOCAL_ONLY,
    },
    Row {
        id: "timers",
        name: "Timers",
        summary: "Starts a timer in Krovi's Timers tool.",
        status: IntegrationStatus::Builtin,
        data: "The duration and an optional name, stored on this PC.",
        network: LOCAL_ONLY,
    },
    Row {
        id: "reminders",
        name: "Reminders",
        summary: "Adds a reminder to Krovi's calendar; a Windows notification rings at that time.",
        status: IntegrationStatus::Builtin,
        data: "The reminder text, date and time, stored on this PC.",
        network: LOCAL_ONLY,
    },
    Row {
        id: "notes",
        name: "Notes",
        summary: "Saves a note in Krovi's Notes tool.",
        status: IntegrationStatus::Builtin,
        data: "The note text, stored on this PC.",
        network: LOCAL_ONLY,
    },
    Row {
        id: "gmail-compose",
        name: "Gmail draft",
        summary: "Opens a Gmail compose window with the recipient, subject and message filled in. You press Send in Gmail. Krovi never sends or reads email.",
        status: IntegrationStatus::Builtin,
        data: "The recipient, subject and message you approved, inside the link your browser opens. Krovi cannot see your mailbox.",
        network: "Your browser opens mail.google.com. Krovi has no access to your Gmail account.",
    },
    Row {
        id: "outlook-compose",
        name: "Outlook draft",
        summary: "Opens an Outlook web compose window with the recipient, subject and message filled in. You press Send in Outlook. Krovi never sends or reads email.",
        status: IntegrationStatus::Builtin,
        data: "The recipient, subject and message you approved, inside the link your browser opens. Krovi cannot see your mailbox.",
        network: "Your browser opens outlook.live.com. Krovi has no access to your Outlook account.",
    },
    Row {
        id: "mailto",
        name: "Default mail app draft",
        summary: "Opens a draft in whatever mail app Windows uses by default. You press Send there. Krovi never sends or reads email.",
        status: IntegrationStatus::Builtin,
        data: "The recipient, subject and message you approved, handed to your mail app.",
        network: "Whatever your mail app does when you send. Krovi sends nothing itself.",
    },
    Row {
        id: "google-calendar",
        name: "Google Calendar",
        summary: "Not available yet. Krovi has no Google sign-in, so it cannot see or change your Google calendar.",
        status: IntegrationStatus::ComingSoon,
        data: "None. Nothing is connected.",
        network: "None.",
    },
    Row {
        id: "mcp-servers",
        name: "MCP servers",
        summary: "Registry only. You can record MCP servers here, but Krovi does not start them or talk to them yet.",
        status: IntegrationStatus::ComingSoon,
        data: "A server's name and how to start or reach it, stored on this PC as plain text.",
        network: "None. Krovi never contacts a registered server.",
    },
];

/// The catalog plus one `NotConnected` entry per registered MCP server.
pub fn integrations(mcp: &[McpServer]) -> Vec<Integration> {
    let mut out: Vec<Integration> = CATALOG
        .iter()
        .map(|r| Integration {
            id: r.id.into(),
            name: r.name.into(),
            summary: r.summary.into(),
            status: r.status,
            data: r.data.into(),
            network: r.network.into(),
        })
        .collect();
    for s in mcp {
        let how = match &s.transport {
            McpTransport::Stdio { command, .. } => format!("a local program ({command})"),
            McpTransport::Http { url } => format!("an HTTP address ({url})"),
        };
        out.push(Integration {
            id: format!("mcp:{}", s.id),
            name: s.name.clone(),
            summary: format!(
                "Registered MCP server, reached through {how}. Not connected: Krovi does not start or call MCP servers yet."
            ),
            status: IntegrationStatus::NotConnected,
            data: "Its name and how to start or reach it, stored on this PC as plain text.".into(),
            network: "None. Krovi never contacts a registered server.".into(),
        });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// the MCP registry
// ---------------------------------------------------------------------------------------------

/// How a server would be reached. Only recorded, never used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum McpTransport {
    /// A local program that speaks MCP over stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// A server reached over HTTP(S).
    Http { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    /// Assigned on first save; leave empty for a new server.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    /// Off unless the user turns it on. Even when on, nothing runs it yet.
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    NameEmpty,
    NameTooLong,
    NameBad,
    NameTaken,
    CommandEmpty,
    CommandTooLong,
    CommandBad,
    CommandHasSpaces,
    TooManyArgs,
    ArgBad,
    Url(String),
    UrlNotSecure,
    TooMany,
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McpError::NameEmpty => write!(f, "Give the server a name."),
            McpError::NameTooLong => write!(f, "That name is too long (over {MAX_MCP_NAME_CHARS} characters)."),
            McpError::NameBad => write!(f, "The name cannot contain control characters."),
            McpError::NameTaken => write!(f, "A server with that name is already registered."),
            McpError::CommandEmpty => write!(f, "Enter the program that starts the server."),
            McpError::CommandTooLong => write!(f, "That command is too long."),
            McpError::CommandBad => write!(
                f,
                "The command cannot contain control characters or the symbols & | ; < > ^ \" % $ `."
            ),
            McpError::CommandHasSpaces => {
                write!(f, "Put only the program in Command. Arguments go in the Arguments list.")
            }
            McpError::TooManyArgs => write!(f, "At most {MAX_MCP_ARGS} arguments."),
            McpError::ArgBad => write!(
                f,
                "An argument is empty, too long (over {MAX_MCP_ARG_CHARS} characters), or has a control character."
            ),
            McpError::Url(why) => write!(f, "{why}"),
            McpError::UrlNotSecure => {
                write!(f, "Use https:// for a remote server. Plain http is only allowed for localhost.")
            }
            McpError::TooMany => write!(f, "At most {MAX_MCP_SERVERS} servers can be registered."),
        }
    }
}

impl std::error::Error for McpError {}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// `C:\Program Files\tool\server.exe`
fn is_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Check one server and return it normalised (name tidied, address normalised). The id is left
/// as given.
pub fn validate_server(s: &McpServer) -> Result<McpServer, McpError> {
    let name = s.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(McpError::NameEmpty);
    }
    if has_control(&s.name) {
        return Err(McpError::NameBad);
    }
    if name.chars().count() > MAX_MCP_NAME_CHARS {
        return Err(McpError::NameTooLong);
    }
    let transport = match &s.transport {
        McpTransport::Stdio { command, args } => {
            let command = command.trim();
            if command.is_empty() {
                return Err(McpError::CommandEmpty);
            }
            if command.chars().count() > MAX_MCP_COMMAND_CHARS {
                return Err(McpError::CommandTooLong);
            }
            if has_control(command)
                || command
                    .chars()
                    .any(|c| matches!(c, '&' | '|' | ';' | '<' | '>' | '^' | '"' | '%' | '$' | '`'))
            {
                return Err(McpError::CommandBad);
            }
            if command.chars().any(char::is_whitespace) && !is_drive_path(command) {
                return Err(McpError::CommandHasSpaces);
            }
            if args.len() > MAX_MCP_ARGS {
                return Err(McpError::TooManyArgs);
            }
            if args
                .iter()
                .any(|a| a.is_empty() || a.chars().count() > MAX_MCP_ARG_CHARS || has_control(a))
            {
                return Err(McpError::ArgBad);
            }
            McpTransport::Stdio {
                command: command.to_string(),
                args: args.clone(),
            }
        }
        McpTransport::Http { url: raw } => {
            let v = url::validate_full(raw).map_err(|e| McpError::Url(e.to_string()))?;
            if !v.https && !url::is_loopback(&v.host) {
                return Err(McpError::UrlNotSecure);
            }
            McpTransport::Http { url: v.url }
        }
    };
    Ok(McpServer {
        id: s.id.clone(),
        name,
        transport,
        enabled: s.enabled,
    })
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-');
    let t = if t.is_empty() { "server" } else { t };
    t.chars()
        .take(32)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

/// The user's registered servers. Order is insertion order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct McpRegistry {
    servers: Vec<McpServer>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the stored JSON array. Lenient on purpose: bad JSON gives an empty registry, and an
    /// entry that no longer validates (or repeats an id or name) is dropped, so a hand-edited or
    /// older value can never wedge the feature.
    pub fn from_json(raw: Option<&str>) -> Self {
        let values: Vec<serde_json::Value> = raw
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default();
        let parsed = values
            .into_iter()
            .filter_map(|v| serde_json::from_value::<McpServer>(v).ok());
        let mut reg = McpRegistry::new();
        for item in parsed {
            if item.id.is_empty() || reg.servers.iter().any(|s| s.id == item.id) {
                continue;
            }
            let Ok(clean) = validate_server(&item) else {
                continue;
            };
            if reg
                .servers
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&clean.name))
            {
                continue;
            }
            if reg.servers.len() < MAX_MCP_SERVERS {
                reg.servers.push(clean);
            }
        }
        reg
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.servers).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn list(&self) -> &[McpServer] {
        &self.servers
    }

    fn fresh_id(&self, name: &str) -> String {
        let base = format!("mcp-{}", slug(name));
        let mut id = base.clone();
        let mut n = 2;
        while self.servers.iter().any(|s| s.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Add a server, or update the one with the same id. Returns the stored server.
    pub fn upsert(&mut self, input: McpServer) -> Result<McpServer, McpError> {
        let clean = validate_server(&input)?;
        let index = self
            .servers
            .iter()
            .position(|s| !input.id.is_empty() && s.id == input.id);
        let taken = self
            .servers
            .iter()
            .enumerate()
            .any(|(i, s)| Some(i) != index && s.name.eq_ignore_ascii_case(&clean.name));
        if taken {
            return Err(McpError::NameTaken);
        }
        match index {
            Some(i) => {
                self.servers[i] = clean.clone();
                Ok(clean)
            }
            None => {
                if self.servers.len() >= MAX_MCP_SERVERS {
                    return Err(McpError::TooMany);
                }
                let stored = McpServer {
                    id: self.fresh_id(&clean.name),
                    ..clean
                };
                self.servers.push(stored.clone());
                Ok(stored)
            }
        }
    }

    /// Remove a server by id. `false` if there was none.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.servers.len();
        self.servers.retain(|s| s.id != id);
        self.servers.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(name: &str, command: &str, args: &[&str]) -> McpServer {
        McpServer {
            id: String::new(),
            name: name.into(),
            transport: McpTransport::Stdio {
                command: command.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
            },
            enabled: false,
        }
    }

    fn http(name: &str, url: &str) -> McpServer {
        McpServer {
            id: String::new(),
            name: name.into(),
            transport: McpTransport::Http { url: url.into() },
            enabled: false,
        }
    }

    #[test]
    fn valid_servers_are_normalised() {
        let ok = validate_server(&stdio(
            "  My   Files  ",
            " npx ",
            &["-y", "@scope/server-files", "C:\\Users\\me"],
        ))
        .unwrap();
        assert_eq!(ok.name, "My Files");
        assert_eq!(
            ok.transport,
            McpTransport::Stdio {
                command: "npx".into(),
                args: vec![
                    "-y".into(),
                    "@scope/server-files".into(),
                    "C:\\Users\\me".into()
                ]
            }
        );
        assert!(!ok.enabled);

        let ok = validate_server(&http("Docs", "HTTPS://Mcp.Example.com/v1")).unwrap();
        assert_eq!(
            ok.transport,
            McpTransport::Http {
                url: "https://mcp.example.com/v1".into()
            }
        );
        assert!(validate_server(&http("Local", "http://localhost:8080/mcp")).is_ok());
        assert!(validate_server(&http("Local2", "http://127.0.0.1:8080")).is_ok());
        assert!(
            validate_server(&stdio("Tool", "C:\\Program Files\\Tool\\server.exe", &[])).is_ok()
        );
        assert!(validate_server(&stdio("Tool", "python", &[])).is_ok());
    }

    #[test]
    fn invalid_names() {
        assert_eq!(
            validate_server(&stdio("   ", "x", &[])),
            Err(McpError::NameEmpty)
        );
        assert_eq!(
            validate_server(&stdio(&"n".repeat(41), "x", &[])),
            Err(McpError::NameTooLong)
        );
        assert!(validate_server(&stdio(&"n".repeat(40), "x", &[])).is_ok());
        assert_eq!(
            validate_server(&stdio("bad\u{0}name", "x", &[])),
            Err(McpError::NameBad)
        );
        assert_eq!(
            validate_server(&stdio("two\nlines", "x", &[])),
            Err(McpError::NameBad)
        );
    }

    #[test]
    fn invalid_commands_and_args() {
        assert_eq!(
            validate_server(&stdio("a", "", &[])),
            Err(McpError::CommandEmpty)
        );
        assert_eq!(
            validate_server(&stdio("a", "   ", &[])),
            Err(McpError::CommandEmpty)
        );
        assert_eq!(
            validate_server(&stdio("a", "npx -y server", &[])),
            Err(McpError::CommandHasSpaces)
        );
        for bad in [
            "a && b",
            "a;b",
            "a|b",
            "a>b",
            "a<b",
            "\"a\"",
            "%APPDATA%\\x",
            "$(whoami)",
            "a`b`",
            "a^b",
            "a\nb",
        ] {
            assert_eq!(
                validate_server(&stdio("a", bad, &[])),
                Err(McpError::CommandBad),
                "{bad:?}"
            );
        }
        assert_eq!(
            validate_server(&stdio("a", &"c".repeat(261), &[])),
            Err(McpError::CommandTooLong)
        );
        assert_eq!(
            validate_server(&stdio("a", "x", &[""])),
            Err(McpError::ArgBad)
        );
        assert_eq!(
            validate_server(&stdio("a", "x", &["ok\nnot ok"])),
            Err(McpError::ArgBad)
        );
        assert_eq!(
            validate_server(&stdio("a", "x", &[&"a".repeat(501)])),
            Err(McpError::ArgBad)
        );
        let many: Vec<&str> = vec!["a"; 33];
        assert_eq!(
            validate_server(&stdio("a", "x", &many)),
            Err(McpError::TooManyArgs)
        );
        let max: Vec<&str> = vec!["a"; 32];
        assert!(validate_server(&stdio("a", "x", &max)).is_ok());
        // arguments may hold anything printable: they are never given to a shell
        assert!(validate_server(&stdio("a", "x", &["--flag=a b", "&", ">"])).is_ok());
    }

    #[test]
    fn invalid_urls() {
        for (url, want) in [
            ("http://example.com/mcp", McpError::UrlNotSecure),
            ("http://10.0.0.5:3000", McpError::UrlNotSecure),
        ] {
            assert_eq!(validate_server(&http("a", url)), Err(want), "{url}");
        }
        for bad in [
            "",
            "example.com",
            "javascript:alert(1)",
            "file:///C:/x",
            "ftp://example.com",
            "https://user:pw@example.com",
            "https://exa mple.com",
            "https://example",
            "wss://example.com/mcp",
        ] {
            assert!(
                matches!(validate_server(&http("a", bad)), Err(McpError::Url(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn upsert_assigns_unique_ids_and_updates_in_place() {
        let mut reg = McpRegistry::new();
        let a = reg.upsert(stdio("My Files", "npx", &[])).unwrap();
        assert_eq!(a.id, "mcp-my-files");
        assert!(!a.enabled);
        let b = reg
            .upsert(http("my   FILES", "https://x.example.com"))
            .unwrap_err();
        assert_eq!(
            b,
            McpError::NameTaken,
            "names are unique ignoring case and repeated spaces"
        );
        let c = reg
            .upsert(http("My Files 2", "https://x.example.com"))
            .unwrap();
        assert_eq!(c.id, "mcp-my-files-2");
        let d = reg
            .upsert(http("My Files!", "https://y.example.com"))
            .unwrap();
        assert_eq!(
            d.id, "mcp-my-files-3",
            "same slug, different name: suffix keeps ids unique"
        );

        // update by id, including renaming and toggling
        let mut edit = a.clone();
        edit.name = "Renamed".into();
        edit.enabled = true;
        let saved = reg.upsert(edit).unwrap();
        assert_eq!(
            (saved.id.as_str(), saved.name.as_str(), saved.enabled),
            ("mcp-my-files", "Renamed", true)
        );
        assert_eq!(reg.list().len(), 3);
        assert_eq!(reg.list()[0].name, "Renamed", "stays in place");
        // saving a server under its own name again is not a clash
        let again = reg.list()[0].clone();
        assert!(reg.upsert(again).is_ok());
        // but taking another server's name is
        let mut steal = reg.list()[0].clone();
        steal.name = "my files 2".into();
        assert_eq!(reg.upsert(steal), Err(McpError::NameTaken));
    }

    #[test]
    fn an_unknown_id_creates_a_new_server_with_a_fresh_id() {
        let mut reg = McpRegistry::new();
        let mut s = stdio("Ghost", "x", &[]);
        s.id = "mcp-made-up".into();
        let stored = reg.upsert(s).unwrap();
        assert_eq!(stored.id, "mcp-ghost");
    }

    #[test]
    fn invalid_input_changes_nothing() {
        let mut reg = McpRegistry::new();
        reg.upsert(stdio("Good", "x", &[])).unwrap();
        let before = reg.clone();
        assert!(reg.upsert(stdio("Bad", "a && b", &[])).is_err());
        assert!(reg.upsert(http("Bad2", "javascript:1")).is_err());
        assert_eq!(reg, before);
    }

    #[test]
    fn the_registry_is_capped() {
        let mut reg = McpRegistry::new();
        for i in 0..MAX_MCP_SERVERS {
            reg.upsert(stdio(&format!("server {i}"), "x", &[])).unwrap();
        }
        assert_eq!(
            reg.upsert(stdio("one too many", "x", &[])),
            Err(McpError::TooMany)
        );
        // updating an existing one still works when full
        let first = reg.list()[0].clone();
        assert!(reg.upsert(first).is_ok());
    }

    #[test]
    fn remove() {
        let mut reg = McpRegistry::new();
        let a = reg.upsert(stdio("A", "x", &[])).unwrap();
        assert!(reg.remove(&a.id));
        assert!(!reg.remove(&a.id));
        assert!(reg.list().is_empty());
    }

    #[test]
    fn json_roundtrip_and_wire_shape() {
        let mut reg = McpRegistry::new();
        reg.upsert(stdio("Files", "npx", &["-y", "srv"])).unwrap();
        reg.upsert(http("Docs", "https://mcp.example.com")).unwrap();
        let json = reg.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v[0],
            serde_json::json!({ "id": "mcp-files", "name": "Files", "transport": { "type": "stdio", "command": "npx", "args": ["-y", "srv"] }, "enabled": false })
        );
        assert_eq!(
            v[1]["transport"],
            serde_json::json!({ "type": "http", "url": "https://mcp.example.com" })
        );
        assert_eq!(McpRegistry::from_json(Some(&json)), reg);
    }

    #[test]
    fn enabled_defaults_to_false() {
        let s: McpServer =
            serde_json::from_str(r#"{"name":"x","transport":{"type":"stdio","command":"x"}}"#)
                .unwrap();
        assert!(!s.enabled);
        assert_eq!(s.id, "");
        assert_eq!(
            s.transport,
            McpTransport::Stdio {
                command: "x".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn loading_is_lenient() {
        assert!(McpRegistry::from_json(None).list().is_empty());
        assert!(McpRegistry::from_json(Some("not json")).list().is_empty());
        assert!(McpRegistry::from_json(Some("{}")).list().is_empty());
        let stored = r#"[
            {"id":"a","name":"Good","transport":{"type":"stdio","command":"x"},"enabled":true},
            {"id":"a","name":"Duplicate id","transport":{"type":"stdio","command":"x"}},
            {"id":"b","name":"good","transport":{"type":"stdio","command":"x"}},
            {"id":"c","name":"Bad command","transport":{"type":"stdio","command":"a && b"}},
            {"id":"","name":"No id","transport":{"type":"stdio","command":"x"}},
            {"id":"d","name":"Insecure","transport":{"type":"http","url":"http://example.com"}},
            {"id":"e","name":"Fine","transport":{"type":"http","url":"https://example.com"}},
            {"nonsense": true}
        ]"#;
        let reg = McpRegistry::from_json(Some(stored));
        let names: Vec<&str> = reg.list().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Good", "Fine"]);
        assert!(reg.list()[0].enabled, "a stored value survives");
    }

    #[test]
    fn catalog_is_honest() {
        let all = integrations(&[]);
        let by_id = |id: &str| {
            all.iter()
                .find(|i| i.id == id)
                .unwrap_or_else(|| panic!("{id}"))
        };

        assert_eq!(
            by_id("google-calendar").status,
            IntegrationStatus::ComingSoon
        );
        assert!(by_id("google-calendar")
            .summary
            .contains("Not available yet"));
        assert_eq!(by_id("mcp-servers").status, IntegrationStatus::ComingSoon);
        assert!(by_id("mcp-servers").summary.contains("Registry only"));
        for id in ["gmail-compose", "outlook-compose", "mailto"] {
            let i = by_id(id);
            assert_eq!(i.status, IntegrationStatus::Builtin);
            assert!(i.summary.contains("never sends or reads email"), "{id}");
            assert!(i.summary.contains("press Send"), "{id}");
        }
        for id in [
            "websites",
            "google-search",
            "youtube",
            "apps",
            "timers",
            "reminders",
            "notes",
        ] {
            assert_eq!(by_id(id).status, IntegrationStatus::Builtin);
        }
        // every entry says what it touches and where it goes, and ids are unique
        let mut seen = std::collections::HashSet::new();
        for i in &all {
            assert!(
                !i.name.is_empty()
                    && !i.summary.is_empty()
                    && !i.data.is_empty()
                    && !i.network.is_empty(),
                "{}",
                i.id
            );
            assert!(seen.insert(i.id.clone()), "duplicate {}", i.id);
        }
        // nothing claims to read or send mail, call an API, or run MCP
        for i in &all {
            let t = format!("{} {}", i.summary, i.data).to_lowercase();
            assert!(!t.contains("reads your email"), "{}", i.id);
            assert!(!t.contains("connected to your"), "{}", i.id);
        }
    }

    #[test]
    fn registered_mcp_servers_show_as_not_connected() {
        let mut reg = McpRegistry::new();
        reg.upsert(stdio("Files", "npx", &[])).unwrap();
        reg.upsert(http("Docs", "https://mcp.example.com")).unwrap();
        let all = integrations(reg.list());
        let files = all.iter().find(|i| i.id == "mcp:mcp-files").unwrap();
        assert_eq!(files.status, IntegrationStatus::NotConnected);
        assert!(
            files.summary.contains("Not connected")
                && files.summary.contains("does not start or call")
        );
        assert!(files.network.contains("never contacts"));
        let docs = all.iter().find(|i| i.id == "mcp:mcp-docs").unwrap();
        assert_eq!(docs.status, IntegrationStatus::NotConnected);
        assert!(docs.summary.contains("https://mcp.example.com"));
        assert_eq!(all.len(), integrations(&[]).len() + 2);
    }

    #[test]
    fn integration_json_shape() {
        let all = integrations(&[]);
        let v = serde_json::to_value(&all[0]).unwrap();
        assert_eq!(v["status"], "builtin");
        assert!(v["summary"].is_string() && v["data"].is_string() && v["network"].is_string());
        assert_eq!(
            serde_json::to_value(IntegrationStatus::ComingSoon).unwrap(),
            "coming-soon"
        );
        assert_eq!(
            serde_json::to_value(IntegrationStatus::NotConnected).unwrap(),
            "not-connected"
        );
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("My Files"), "my-files");
        assert_eq!(slug("  !!  "), "server");
        assert_eq!(slug("A/B\\C"), "a-b-c");
        assert!(slug(&"x".repeat(100)).len() <= 32);
    }

    #[test]
    fn settings_key_is_the_documented_one() {
        assert_eq!(MCP_SETTINGS_KEY, "agent.mcp.v1");
    }
}
