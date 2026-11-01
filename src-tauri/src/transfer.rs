//! Quick Transfer (owner: transfer). See docs/CONTRACTS.md section 6.
//!
//! A thin Tauri layer over the `krovi-transfer` crate: it owns the single `TransferManager`,
//! turns crate events into Tauri events, and exposes the commands. All behaviour (server,
//! tokens, expiry, filename safety) lives in the crate and is unit/integration tested there.
//!
//! Commands: transfer_share, transfer_receive, transfer_stop, transfer_status (contract);
//! additive: transfer_pick_files, transfer_get_config, transfer_set_config,
//! transfer_open_folder, transfer_reveal, transfer_firewall_status, transfer_firewall_allow.
//! Events: transfer-progress (throttled to ~10/s), transfer-done, transfer-closed (contract);
//! additive: transfer-reached, transfer-failed.

use krovi_transfer::{
    firewall, net, Config, Event, EventSink, ShareInfo, ShareOptions, TransferManager,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, State};

struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: Event) {
        let _ = self.0.emit(event.name(), event.payload());
    }
}

pub struct TransferState {
    mgr: TransferManager,
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let sink = Arc::new(TauriSink(app.handle().clone()));
    app.manage(TransferState {
        mgr: TransferManager::new(Config::default(), sink),
    });

    // "Everything closes on app quit": a tiny inline plugin whose only job is to listen for
    // the runtime's Exit event and close every share (and with it the listening socket).
    let closer = tauri::plugin::Builder::<tauri::Wry, ()>::new("krovi-transfer")
        .on_event(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<TransferState>() {
                    state.mgr.shutdown();
                }
            }
        })
        .build();
    app.handle().plugin(closer)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract commands
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareArgs {
    /// Require a 4-digit PIN for this share (default: the configured setting).
    pub pin: Option<bool>,
    /// Lifetime override in seconds (clamped to 1 minute .. 6 hours).
    pub ttl_secs: Option<u64>,
}

impl ShareArgs {
    fn into_opts(self) -> ShareOptions {
        ShareOptions {
            pin: self.pin,
            ttl_secs: self.ttl_secs.map(|s| s.clamp(60, 6 * 3600)),
        }
    }
}

#[tauri::command]
pub async fn transfer_share(
    state: State<'_, TransferState>,
    paths: Vec<String>,
    opts: Option<ShareArgs>,
) -> Result<ShareInfo, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    state
        .mgr
        .share_files(&paths, opts.unwrap_or_default().into_opts())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn transfer_receive(
    state: State<'_, TransferState>,
    opts: Option<ShareArgs>,
) -> Result<ShareInfo, String> {
    state
        .mgr
        .receive(opts.unwrap_or_default().into_opts())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn transfer_stop(
    state: State<'_, TransferState>,
    id: Option<String>,
) -> Result<(), String> {
    state.mgr.stop(id.as_deref());
    Ok(())
}

#[derive(Serialize)]
pub struct Status {
    pub active: Vec<ShareInfo>,
}

#[tauri::command]
pub async fn transfer_status(state: State<'_, TransferState>) -> Result<Status, String> {
    Ok(Status {
        active: state.mgr.status(),
    })
}

// ---------------------------------------------------------------------------
// Native file dialog (with a real path, which a webview <input type=file> cannot give us)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn transfer_pick_files(window: tauri::WebviewWindow) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        krovi_transfer::pick_files(Some(&window))
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Settings (in memory; the UI persists them and re-applies on start)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferConfig {
    /// Per-file and per-receive-share cap in bytes (default 2 GB).
    pub max_upload_bytes: u64,
    /// Share lifetime in seconds (default 10 minutes).
    pub ttl_secs: u64,
    /// Ask for a 4-digit PIN on new shares (default off).
    pub require_pin: bool,
    /// Read-only: where phone uploads are saved.
    #[serde(default)]
    pub receive_dir: String,
}

fn to_view(c: &Config) -> TransferConfig {
    TransferConfig {
        max_upload_bytes: c.max_upload_bytes,
        ttl_secs: c.ttl.as_secs(),
        require_pin: c.require_pin,
        receive_dir: c.receive_dir.to_string_lossy().into_owned(),
    }
}

#[tauri::command]
pub async fn transfer_get_config(
    state: State<'_, TransferState>,
) -> Result<TransferConfig, String> {
    Ok(to_view(&state.mgr.config()))
}

#[tauri::command]
pub async fn transfer_set_config(
    state: State<'_, TransferState>,
    config: TransferConfig,
) -> Result<TransferConfig, String> {
    let mut cfg = state.mgr.config();
    cfg.max_upload_bytes = config
        .max_upload_bytes
        .clamp(1024 * 1024, 100 * 1024 * 1024 * 1024);
    cfg.ttl = Duration::from_secs(config.ttl_secs.clamp(60, 6 * 3600));
    cfg.require_pin = config.require_pin;
    let out = to_view(&cfg);
    state.mgr.set_config(cfg);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Received files: open the folder / reveal a file (only inside the receive folder)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn explorer(raw_args: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer.exe")
        .raw_arg(raw_args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open Explorer: {e}"))
}

#[cfg(not(windows))]
fn explorer(_raw_args: &str) -> Result<(), String> {
    Err("Only available on Windows.".into())
}

#[tauri::command]
pub async fn transfer_open_folder(state: State<'_, TransferState>) -> Result<(), String> {
    let dir = state.mgr.config().receive_dir;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    explorer(&format!("\"{}\"", dir.display()))
}

fn inside(dir: &Path, path: &Path) -> bool {
    match (std::fs::canonicalize(dir), std::fs::canonicalize(path)) {
        (Ok(d), Ok(p)) => p.starts_with(d),
        _ => false,
    }
}

#[tauri::command]
pub async fn transfer_reveal(state: State<'_, TransferState>, path: String) -> Result<(), String> {
    let dir = state.mgr.config().receive_dir;
    let p = PathBuf::from(&path);
    if !inside(&dir, &p) {
        return Err("That file is not in the Krovi receive folder.".into());
    }
    explorer(&format!("/select,\"{}\"", p.display()))
}

// ---------------------------------------------------------------------------
// Firewall help (read-only status; the rule is only added by an explicit click + UAC prompt)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallReport {
    pub lan_ip: Option<String>,
    pub adapter: Option<String>,
    pub rule_installed: bool,
    /// "Private" | "Public" | "DomainAuthenticated" when Windows reports it.
    pub profile: Option<String>,
    pub exe: String,
}

async fn report() -> Result<FirewallReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let lan = net::detect();
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let st = firewall::run_status(&exe, lan.as_ref().map(|l| l.adapter.as_str()));
        Ok(FirewallReport {
            lan_ip: lan.as_ref().map(|l| l.ip.to_string()),
            adapter: lan.map(|l| l.adapter),
            rule_installed: st.rule_installed,
            profile: st.profile,
            exe: exe.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn transfer_firewall_status() -> Result<FirewallReport, String> {
    report().await
}

/// Adds the scoped Private-profile rule for this exe. Windows shows a UAC prompt; declining it
/// returns an error and changes nothing. Only the explicit UI button calls this.
#[tauri::command]
pub async fn transfer_firewall_allow() -> Result<FirewallReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        firewall::run_allow(&exe)
    })
    .await
    .map_err(|e| e.to_string())??;
    report().await
}
