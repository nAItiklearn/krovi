//! One error type for the whole crate. Messages are written for a human: the Tauri layer
//! forwards them to the UI verbatim.

use std::fmt;
use std::net::Ipv4Addr;

#[derive(Debug)]
pub enum TransferError {
    /// No private (RFC 1918) LAN interface is up. Quick Transfer never falls back to 0.0.0.0.
    NoLan,
    /// The caller passed no files.
    NoFiles,
    /// A path cannot be shared (missing, a folder, unreadable...).
    BadFile(String),
    /// The OS refused a listening socket.
    Bind(String),
    /// The OS random source failed (never expected; we refuse to continue with weak tokens).
    Entropy(String),
    /// The LAN address changed while shares are open; stop them first.
    NetworkChanged { had: Ipv4Addr, now: Ipv4Addr },
    /// A caller-provided bind override is not acceptable (0.0.0.0, multicast...).
    BadBind(Ipv4Addr),
    /// Filesystem trouble preparing the receive folder.
    Io(String),
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransferError::NoLan => write!(
                f,
                "No Wi-Fi or LAN network found. Connect this PC to the same Wi-Fi as your phone and try again."
            ),
            TransferError::NoFiles => write!(f, "Choose at least one file to share."),
            TransferError::BadFile(m) => write!(f, "{m}"),
            TransferError::Bind(m) => write!(f, "Could not start the local transfer server: {m}"),
            TransferError::Entropy(m) => {
                write!(f, "The system random generator is unavailable ({m}); not sharing.")
            }
            TransferError::NetworkChanged { had, now } => write!(
                f,
                "Your network changed ({had} to {now}). Stop the open shares and start again."
            ),
            TransferError::BadBind(ip) => write!(f, "{ip} is not an acceptable address to listen on."),
            TransferError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for TransferError {}

impl From<std::io::Error> for TransferError {
    fn from(e: std::io::Error) -> Self {
        TransferError::Io(e.to_string())
    }
}
