//! Jira Cloud commands. The API token lives in the system keyring.

use std::collections::BTreeMap;
use std::path::Path;

use adjutant_core::config::{JiraConfig, VaultConfig};
use adjutant_core::jira::{self, JiraCache};
use adjutant_core::secrets;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, CmdResult, aerr, blocking, err};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSettings {
    base_url: String,
    email: String,
    queries: BTreeMap<String, String>,
    #[serde(default)]
    has_token: bool,
}

#[tauri::command]
pub async fn jira_settings(state: State<'_, AppState>) -> CmdResult<JiraSettings> {
    let v = state.vault()?;
    blocking(move || {
        let cfg = VaultConfig::load(&v)?.jira.unwrap_or_default();
        Ok(JiraSettings {
            base_url: cfg.base_url,
            email: cfg.email,
            queries: cfg.queries,
            has_token: secrets::get_secret(secrets::JIRA_TOKEN)
                .ok()
                .flatten()
                .is_some(),
        })
    })
    .await
}

#[tauri::command]
pub async fn jira_save_settings(
    state: State<'_, AppState>,
    settings: JiraSettings,
    token: Option<String>,
) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || {
        let existing = VaultConfig::load(&v)?.jira.unwrap_or_default();
        let cfg = JiraConfig {
            base_url: settings.base_url,
            email: settings.email,
            queries: settings.queries,
            sync_minutes: existing.sync_minutes,
        };
        VaultConfig::set_jira(&v, &cfg)?;
        if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
            secrets::set_secret(secrets::JIRA_TOKEN, t.trim())?;
        }
        Ok(())
    })
    .await
}

async fn config_and_token(vault: &Path) -> CmdResult<(JiraConfig, String)> {
    let v = vault.to_path_buf();
    blocking(move || {
        let cfg = VaultConfig::load(&v)?.jira.ok_or_else(|| {
            anyhow::anyhow!("Jira is not configured. Open Settings to connect it.")
        })?;
        let token = secrets::get_secret(secrets::JIRA_TOKEN)?.ok_or_else(|| {
            anyhow::anyhow!("No Jira API token in the keyring. Add it in Settings.")
        })?;
        Ok((cfg, token))
    })
    .await
}

#[tauri::command]
pub async fn jira_test(state: State<'_, AppState>) -> CmdResult<String> {
    let v = state.vault()?;
    let (cfg, token) = config_and_token(&v).await?;
    jira::test_connection(&cfg, &token).await.map_err(aerr)
}

/// Syncs and broadcasts `jira-synced`. Shared with the scheduler.
pub async fn run_sync(app: &AppHandle, vault: &Path) -> Result<JiraCache, String> {
    let (cfg, token) = config_and_token(vault).await?;
    let cache = jira::sync(vault, &cfg, &token).await.map_err(aerr)?;
    let _ = app.emit("jira-synced", &cache);
    Ok(cache)
}

#[tauri::command]
pub async fn jira_sync(app: AppHandle, state: State<'_, AppState>) -> CmdResult<JiraCache> {
    let v = state.vault()?;
    run_sync(&app, &v).await
}

#[tauri::command]
pub async fn jira_cache(state: State<'_, AppState>) -> CmdResult<Option<JiraCache>> {
    let v = state.vault()?;
    blocking(move || jira::load_cache(&v)).await.map_err(err)
}
