//! Vault commands: open, tree, notes, links, search, templates, sessions.

use std::path::PathBuf;

use adjutant_core::config::VaultConfig;
use adjutant_core::{search, vault};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::state::{AppState, CmdResult, blocking};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    path: PathBuf,
    name: String,
    initialized: bool,
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
    *state.watcher.lock().unwrap() = crate::watcher::VaultWatcher::start(app.clone(), &path)
        .map_err(|e| tracing::warn!("file watcher: {e:#}"))
        .ok();
    let mut settings = crate::settings::load(&app);
    settings.last_vault = Some(path.clone());
    crate::settings::save(&app, &settings)?;
    Ok(VaultInfo {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        initialized: vault::is_initialized(&path),
        path,
    })
}

/// The vault to open on start: `ADJUTANT_VAULT`, else the last one used.
#[tauri::command]
pub fn vault_last(app: AppHandle) -> Option<PathBuf> {
    std::env::var_os("ADJUTANT_VAULT")
        .map(PathBuf::from)
        .or_else(|| crate::settings::load(&app).last_vault)
        .filter(|p| p.is_dir())
}

#[tauri::command]
pub async fn vault_scaffold(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let v = state.vault()?;
    blocking(move || vault::scaffold(&v)).await
}

#[tauri::command]
pub async fn vault_tree(state: State<'_, AppState>) -> CmdResult<vault::TreeNode> {
    let v = state.vault()?;
    blocking(move || vault::tree(&v)).await
}

#[tauri::command]
pub async fn note_read(state: State<'_, AppState>, path: String) -> CmdResult<String> {
    let v = state.vault()?;
    blocking(move || vault::read_note(&v, &path)).await
}

#[tauri::command]
pub async fn note_write(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> CmdResult<()> {
    let v = state.vault()?;
    let watcher_skip = path.clone();
    if let Some(w) = state.watcher.lock().unwrap().as_ref() {
        w.ignore_next(&watcher_skip);
    }
    blocking(move || vault::write_note(&v, &path, &content)).await
}

/// Creates a note, optionally from `templates/<template>.md`.
#[tauri::command]
pub async fn note_create(
    state: State<'_, AppState>,
    path: String,
    template: Option<String>,
) -> CmdResult<String> {
    let v = state.vault()?;
    blocking(move || {
        let title = path
            .rsplit('/')
            .next()
            .unwrap_or(&path)
            .trim_end_matches(".md")
            .to_string();
        let content = match template {
            Some(t) => vault::render_template(&v, &t, &title, chrono::Local::now().naive_local())?,
            None => format!("# {title}\n\n"),
        };
        vault::create_note(&v, &path, &content)
    })
    .await
}

#[tauri::command]
pub async fn note_rename(state: State<'_, AppState>, from: String, to: String) -> CmdResult<usize> {
    let v = state.vault()?;
    blocking(move || vault::rename(&v, &from, &to)).await
}

#[tauri::command]
pub async fn note_trash(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || vault::trash(&v, &path)).await
}

#[tauri::command]
pub async fn dir_create(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || vault::create_dir(&v, &path)).await
}

#[tauri::command]
pub async fn notes_list(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let v = state.vault()?;
    blocking(move || vault::list_notes(&v)).await
}

#[tauri::command]
pub async fn backlinks(
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<Vec<vault::Backlink>> {
    let v = state.vault()?;
    blocking(move || vault::backlinks(&v, &path)).await
}

#[tauri::command]
pub async fn search(state: State<'_, AppState>, query: String) -> CmdResult<search::SearchResults> {
    let v = state.vault()?;
    blocking(move || search::search(&v, &query, 200)).await
}

#[tauri::command]
pub async fn templates_list(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let v = state.vault()?;
    blocking(move || vault::list_templates(&v)).await
}

#[tauri::command]
pub async fn repos_list(state: State<'_, AppState>) -> CmdResult<Vec<vault::RepoInfo>> {
    let v = state.vault()?;
    blocking(move || vault::list_repos(&v)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Visibility {
    agent: Option<String>,
    hidden: bool,
}

/// Which agent may read `path`, per `[folders]` in the vault config.
#[tauri::command]
pub async fn note_visibility(state: State<'_, AppState>, path: String) -> CmdResult<Visibility> {
    let v = state.vault()?;
    blocking(move || {
        let cfg = VaultConfig::load(&v)?;
        Ok(match cfg.folder_agent(&path) {
            Some("none") => Visibility {
                agent: None,
                hidden: true,
            },
            Some(agent) => Visibility {
                agent: Some(agent.to_string()),
                hidden: false,
            },
            None => Visibility {
                agent: Some(
                    cfg.agent
                        .default
                        .clone()
                        .unwrap_or_else(|| "the default agent".into()),
                ),
                hidden: false,
            },
        })
    })
    .await
}

// ----- Saved chat sessions: .vault/sessions/<id>.json (gitignored) -----

#[tauri::command]
pub async fn sessions_list(state: State<'_, AppState>) -> CmdResult<Vec<serde_json::Value>> {
    let v = state.vault()?;
    blocking(move || {
        let dir = v.join(".vault/sessions");
        let mut out: Vec<serde_json::Value> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                if let Ok(bytes) = std::fs::read(e.path())
                    && let Ok(mut s) = serde_json::from_slice::<serde_json::Value>(&bytes)
                {
                    // The list only needs metadata.
                    if let Some(o) = s.as_object_mut() {
                        o.remove("items");
                    }
                    out.push(s);
                }
            }
        }
        out.sort_by(|a, b| b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str()));
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn session_save(state: State<'_, AppState>, session: serde_json::Value) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || {
        let id = session["id"].as_str().unwrap_or_default();
        anyhow::ensure!(
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "invalid session id"
        );
        let dir = v.join(".vault/sessions");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_vec(&session)?,
        )?;
        Ok(())
    })
    .await
}
