//! App-level settings (not vault content): `<config dir>/settings.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::state::{CmdResult, err};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub last_vault: Option<PathBuf>,
    pub autostart: bool,
    pub capture_hotkey: String,
    pub close_to_tray: bool,
    /// Ask GitHub releases for app updates (the only request the app itself makes).
    pub check_updates: bool,
    /// Vaults whose first-run setup was completed.
    pub setup_done: Vec<PathBuf>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            last_vault: None,
            autostart: false,
            capture_hotkey: "CommandOrControl+Shift+Space".into(),
            close_to_tray: true,
            check_updates: true,
            setup_done: Vec::new(),
        }
    }
}

fn path(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(err)?
        .join("settings.json"))
}

pub fn load(app: &AppHandle) -> AppSettings {
    path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, s: &AppSettings) -> CmdResult<()> {
    let p = path(app)?;
    std::fs::create_dir_all(p.parent().unwrap()).map_err(err)?;
    std::fs::write(p, serde_json::to_vec_pretty(s).map_err(err)?).map_err(err)
}

/// What the Settings screen edits (everything except the last vault).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    pub autostart: bool,
    pub capture_hotkey: String,
    pub close_to_tray: bool,
    pub check_updates: bool,
    /// Set when the global shortcut could not be registered (e.g. Wayland).
    #[serde(default, skip_deserializing)]
    pub hotkey_error: Option<String>,
    /// The shortcut as registered, e.g. "Ctrl+Shift+Space".
    #[serde(default, skip_deserializing)]
    pub hotkey_label: Option<String>,
    /// The desktop owns the binding (Wayland); change it there.
    #[serde(default, skip_deserializing)]
    pub hotkey_managed: bool,
}

#[tauri::command]
pub fn app_settings(app: AppHandle) -> AppSettingsView {
    let s = load(&app);
    AppSettingsView {
        autostart: s.autostart,
        capture_hotkey: s.capture_hotkey,
        close_to_tray: s.close_to_tray,
        check_updates: s.check_updates,
        hotkey_error: crate::desktop::hotkey_error(),
        hotkey_label: crate::desktop::hotkey_label(),
        hotkey_managed: crate::desktop::hotkey_managed_by_desktop(),
    }
}

#[tauri::command]
pub fn app_save_settings(app: AppHandle, settings: AppSettingsView) -> CmdResult<()> {
    let mut s = load(&app);
    let hotkey_changed = s.capture_hotkey != settings.capture_hotkey;
    s.autostart = settings.autostart;
    s.capture_hotkey = settings.capture_hotkey;
    s.close_to_tray = settings.close_to_tray;
    s.check_updates = settings.check_updates;
    save(&app, &s)?;
    crate::desktop::apply_autostart(&app, s.autostart);
    if hotkey_changed {
        crate::desktop::register_capture_hotkey(&app, &s.capture_hotkey)?;
    }
    Ok(())
}
