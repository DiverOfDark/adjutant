//! Shared application state.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use adjutant_acp::AgentHandle;

use crate::audit::AuditLog;

pub type CmdResult<T> = Result<T, String>;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Formats an anyhow error with its causes.
pub fn aerr(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[derive(Default)]
pub struct AppState {
    vault: RwLock<Option<PathBuf>>,
    pub agent: tokio::sync::Mutex<Option<Arc<AgentHandle>>>,
    pub audit: Arc<AuditLog>,
    /// Ids of scheduled jobs currently running.
    pub running_jobs: Mutex<HashSet<String>>,
    pub watcher: Mutex<Option<crate::watcher::VaultWatcher>>,
}

impl AppState {
    pub fn vault(&self) -> CmdResult<PathBuf> {
        self.vault_opt()
            .ok_or_else(|| "no vault is open".to_string())
    }

    pub fn vault_opt(&self) -> Option<PathBuf> {
        self.vault.read().unwrap().clone()
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

/// Runs blocking vault work off the async runtime.
pub async fn blocking<T, F>(f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(err)?
        .map_err(aerr)
}
