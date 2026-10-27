//! Events the ACP layer reports to the app (UI, audit log, scheduler).
//!
//! Protocol payloads are forwarded as JSON (`serde_json::Value`) so the
//! frontend renders the ACP shapes directly and the rest of the app does not
//! depend on the pinned crate's Rust types.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentEvent {
    /// `initialize` succeeded.
    #[serde(rename_all = "camelCase")]
    Ready {
        agent_info: serde_json::Value,
        agent_capabilities: serde_json::Value,
    },
    /// A `session/update` notification (message chunk, tool call, plan, ...).
    #[serde(rename_all = "camelCase")]
    SessionUpdate {
        session_id: String,
        update: serde_json::Value,
    },
    /// The agent asks for permission; answer with `AgentHandle::respond_permission`.
    #[serde(rename_all = "camelCase")]
    PermissionRequest {
        request_id: u64,
        session_id: String,
        tool_call: serde_json::Value,
        options: serde_json::Value,
    },
    /// A permission request was decided by policy or by the user.
    #[serde(rename_all = "camelCase")]
    PermissionDecided {
        request_id: u64,
        option_id: Option<String>,
        automatic: bool,
    },
    #[serde(rename_all = "camelCase")]
    FileRead {
        session_id: String,
        path: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    FileWritten {
        session_id: String,
        path: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    FileAccessDenied {
        session_id: String,
        path: PathBuf,
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    TerminalStarted {
        session_id: String,
        terminal_id: String,
        command: String,
    },
    #[serde(rename_all = "camelCase")]
    PromptFinished {
        session_id: String,
        stop_reason: serde_json::Value,
    },
    /// A line the agent wrote to stderr (useful for login/auth problems).
    Stderr {
        line: String,
    },
    Error {
        message: String,
    },
    Exited {
        code: Option<i32>,
    },
}

/// Receives events from an agent connection. Must not block.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: AgentEvent);
}

impl<F: Fn(AgentEvent) + Send + Sync + 'static> EventSink for F {
    fn emit(&self, event: AgentEvent) {
        self(event)
    }
}

/// What to do with a permission request before asking the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Ask,
    /// Select the agent's first "allow" option.
    Allow,
    /// Select the agent's first "reject" option.
    Reject,
}

/// Decides permission requests automatically ("always allow" rules from
/// `.vault/config.toml`, scheduled jobs that must not block on a human, ...).
pub trait PermissionPolicy: Send + Sync + 'static {
    /// `tool_call` is the ACP `ToolCallUpdate` as JSON.
    fn decide(&self, agent_id: &str, tool_call: &serde_json::Value) -> PolicyDecision;
}

/// Always asks the user.
pub struct AskAlways;

impl PermissionPolicy for AskAlways {
    fn decide(&self, _agent_id: &str, _tool_call: &serde_json::Value) -> PolicyDecision {
        PolicyDecision::Ask
    }
}
