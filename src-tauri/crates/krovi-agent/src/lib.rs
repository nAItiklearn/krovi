//! Krovi Agent Mode: intent -> plan -> permission -> (execution in the app crate).
//!
//! Pure logic only: no OS calls, no network, no clock (the host passes `now` and the local UTC
//! offset in), no LLM. Everything here is deterministic and unit-tested; the thin Tauri glue in
//! `src-tauri/src/agent.rs` executes what this crate has already validated.
//!
//! Pipeline:
//!
//! 1. [`parse`] turns a sentence into [`Intent`]s (open a site or app, search, timer, reminder,
//!    note, email draft). Several steps can be chained with "and then" / "then" / "and".
//! 2. [`build_plan`] validates each intent and assigns a [`Risk`]: `Safe` runs straight away,
//!    `Confirm` needs the user's explicit OK on a confirmation card, `Blocked` never runs.
//! 3. [`Session`] holds plans and the [`TaskQueue`]; `begin_run` decides which steps may run now
//!    (Safe ones, plus Confirm ones the user confirmed) and the host executes them.
//!
//! Hard rules baked into the types:
//!
//! * Krovi never sends or reads email. [`mail`] only builds a *compose* link; the user presses
//!   Send in their own mail app.
//! * Only `http`/`https` links are ever opened ([`url`]); credentials in a link, `javascript:`,
//!   `file:`, `data:` and UNC paths are refused.
//! * Apps are launched only from the host-supplied Start-menu list.
//! * MCP servers are a *registry only* ([`registry`]); nothing here starts or calls one.

pub mod civil;
pub mod intent;
pub mod mail;
pub mod parse;
pub mod plan;
pub mod registry;
pub mod session;
pub mod sites;
pub mod tasks;
pub mod text;
pub mod url;
pub mod when;

pub use intent::{AppEntry, AppMatch, Intent, MailProvider};
pub use mail::{compose_url, validate_address, EmailDraft, MailError};
pub use parse::{parse, ParseContext};
pub use plan::{build_plan, Action, Plan, Risk, Step};
pub use registry::{
    integrations, Integration, IntegrationStatus, McpError, McpRegistry, McpServer, McpTransport,
    MCP_SETTINGS_KEY,
};
pub use session::{Dispatch, RunOutcome, Session, SessionError};
pub use tasks::{Task, TaskError, TaskQueue, TaskStatus, HISTORY_CAP};
pub use url::{check_openable, encode_component, validate, validate_full, Openable, UrlError};

/// One call for hosts: parse a sentence and build the plan for it.
pub fn plan_text(plan_id: &str, input: &str, ctx: &ParseContext<'_>) -> Plan {
    let intents = parse(input, ctx);
    build_plan(plan_id, &intents, ctx.now_utc_ms)
}
