//! Opening a vault and remembering the last one.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::state::{AppState, CmdResult, err};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    last_vault: Option<PathBuf>,
}

fn settings_path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(err)?
        .join("settings.json"))
}

fn load_settings(app: &AppHandle) -> AppSettings {
    settings_path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &AppSettings) -> CmdResult<()> {
    let path = settings_path(app)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    std::fs::write(path, serde_json::to_vec_pretty(s).map_err(err)?).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    path: PathBuf,
    name: String,
}

/// Opens `path` as the vault (the folder must exist).
#[tauri::command]
pub async fn vault_open(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> CmdResult<VaultInfo> {
    let path = dunce::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    if let Some(agent) = state.agent.lock().await.take() {
        agent.stop();
    }
    state.set_vault(path.clone());
    let mut settings = load_settings(&app);
    settings.last_vault = Some(path.clone());
    save_settings(&app, &settings)?;
    Ok(VaultInfo {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path,
    })
}

/// The vault to open on start: `ADJUTANT_VAULT`, else the last one used.
#[tauri::command]
pub fn vault_last(app: AppHandle) -> Option<PathBuf> {
    std::env::var_os("ADJUTANT_VAULT")
        .map(PathBuf::from)
        .or_else(|| load_settings(&app).last_vault)
        .filter(|p| p.is_dir())
}
