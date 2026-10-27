//! Shared application state.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use adjutant_acp::AgentHandle;

use crate::audit::AuditLog;

pub type CmdResult<T> = Result<T, String>;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Default)]
pub struct AppState {
    vault: RwLock<Option<PathBuf>>,
    pub agent: tokio::sync::Mutex<Option<Arc<AgentHandle>>>,
    pub audit: Arc<AuditLog>,
}

impl AppState {
    pub fn vault(&self) -> CmdResult<PathBuf> {
        self.vault
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| "no vault is open".to_string())
    }

    pub fn set_vault(&self, path: PathBuf) {
        self.audit.set_vault(&path);
        *self.vault.write().unwrap() = Some(path);
    }

    pub async fn current_agent(&self) -> CmdResult<Arc<AgentHandle>> {
        self.agent
            .lock()
            .await
            .clone()
            .ok_or_else(|| "no agent is connected".to_string())
    }
}
