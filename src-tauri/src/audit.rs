//! Append-only audit log of agent tool calls and file access:
//! `.vault/audit.log`, one JSON object per line.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use adjutant_acp::AgentEvent;
use serde_json::json;

#[derive(Default)]
pub struct AuditLog {
    path: Mutex<Option<PathBuf>>,
}

impl AuditLog {
    pub fn set_vault(&self, vault: &Path) {
        *self.path.lock().unwrap() = Some(vault.join(".vault/audit.log"));
    }

    pub fn record(&self, agent: &str, event: &AgentEvent) {
        let detail = match event {
            AgentEvent::SessionUpdate { update, .. } if update["sessionUpdate"] == "tool_call" => {
                json!({"toolCall": update["title"], "kind": update["kind"], "id": update["toolCallId"]})
            }
            AgentEvent::SessionUpdate { .. } | AgentEvent::Stderr { .. } => return,
            AgentEvent::Ready { .. } => json!({"event": "ready"}),
            other => serde_json::to_value(other).unwrap_or_default(),
        };
        self.write(json!({
            "ts": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            "agent": agent,
            "detail": detail,
        }));
    }

    /// Records an app-side action (task archived, Jira synced, job run).
    pub fn record_app(&self, action: &str, detail: serde_json::Value) {
        self.write(json!({
            "ts": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            "app": action,
            "detail": detail,
        }));
    }

    fn write(&self, line: serde_json::Value) {
        let Some(path) = self.path.lock().unwrap().clone() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}
