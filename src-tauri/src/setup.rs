//! First-run setup: what's missing on this machine and in this vault, and
//! the one-click fixes the setup screen offers.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use adjutant_core::agents::{self, Discovery};
use adjutant_core::config::{JobConfig, VaultConfig};
use adjutant_core::{scheduler, vault};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, CmdResult, blocking};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleToggle {
    id: String,
    name: String,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    os: &'static str,
    initialized: bool,
    git_available: bool,
    git_repo: bool,
    mise_version: Option<String>,
    /// Tools pinned in mise.toml that are not installed yet.
    missing_tools: Vec<String>,
    discovery: Discovery,
    hidden_folders: Vec<String>,
    agent_schedules: Vec<ScheduleToggle>,
    jira_configured: bool,
    autostart: bool,
    check_updates: bool,
    wayland: bool,
    capture_command: String,
    hotkey_error: Option<String>,
    hotkey_label: Option<String>,
    hotkey_managed: bool,
    done: bool,
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

fn run_ok(cmd: &mut Command) -> Option<String> {
    let out = cmd.stdin(Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Tool keys from `mise ls --current --json` without an installed version.
fn missing_tools(mise: &Path, vault: &Path) -> Vec<String> {
    let Some(json) = run_ok(
        Command::new(mise)
            .args(["ls", "--current", "--json"])
            .current_dir(vault),
    ) else {
        return Vec::new();
    };
    let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json) else {
        return Vec::new();
    };
    let mut missing: Vec<String> = map
        .into_iter()
        .filter(|(_, versions)| {
            !versions
                .as_array()
                .is_some_and(|vs| vs.iter().any(|v| v["installed"].as_bool() == Some(true)))
        })
        .map(|(tool, _)| tool)
        .collect();
    missing.sort();
    missing
}

#[tauri::command]
pub async fn setup_status(app: AppHandle, state: State<'_, AppState>) -> CmdResult<SetupStatus> {
    let v = state.vault()?;
    let settings = crate::settings::load(&app);
    let hotkey_error = crate::desktop::hotkey_error();
    let hotkey_label = crate::desktop::hotkey_label();
    let hotkey_managed = crate::desktop::hotkey_managed_by_desktop();
    blocking(move || {
        let cfg = VaultConfig::load(&v)?;
        let discovery = agents::discover(&v, &cfg);
        let mise_version = discovery
            .mise_bin
            .as_ref()
            .and_then(|m| run_ok(Command::new(m).arg("--version")));
        let missing = match (&discovery.mise_bin, discovery.mise_untrusted) {
            (Some(m), false) => missing_tools(m, &v),
            _ => Vec::new(),
        };
        let agent_schedules = scheduler::effective_schedules(&cfg)
            .into_iter()
            .filter(|s| matches!(s.job, JobConfig::Agent { .. }))
            .map(|s| ScheduleToggle {
                name: s.name.clone().unwrap_or_else(|| s.id.clone()),
                id: s.id,
                enabled: s.enabled,
            })
            .collect();
        Ok(SetupStatus {
            os: os(),
            initialized: vault::is_initialized(&v),
            git_available: run_ok(Command::new("git").arg("--version")).is_some(),
            git_repo: v.join(".git").exists(),
            mise_version,
            missing_tools: missing,
            hidden_folders: cfg.hidden_folders(),
            agent_schedules,
            jira_configured: cfg.jira.is_some(),
            autostart: settings.autostart,
            check_updates: settings.check_updates,
            wayland: crate::desktop::is_wayland(),
            capture_command: crate::desktop::capture_command(),
            hotkey_error,
            hotkey_label,
            hotkey_managed,
            done: settings.setup_done.iter().any(|p| p == &v),
            discovery,
        })
    })
    .await
}

/// Runs `mise install` in the vault, streaming output as `setup-log` events.
#[tauri::command]
pub async fn setup_install_tools(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || {
        let cfg = VaultConfig::load(&v)?;
        let mise = agents::find_mise(cfg.agent.mise_bin.as_deref())
            .ok_or_else(|| anyhow::anyhow!("mise not found"))?;
        let mut child = Command::new(mise)
            .args(["install", "--yes"])
            .current_dir(&v)
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let ansi = regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap();
        let emit = |line: String| {
            let clean = ansi.replace_all(&line, "").trim().to_string();
            if !clean.is_empty() {
                let _ = app.emit("setup-log", clean);
            }
        };
        let stderr = child.stderr.take().unwrap();
        let app2 = app.clone();
        let ansi2 = ansi.clone();
        let err_thread = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                for part in line.split('\r') {
                    let clean = ansi2.replace_all(part, "").trim().to_string();
                    if !clean.is_empty() {
                        let _ = app2.emit("setup-log", clean);
                    }
                }
            }
        });
        for line in std::io::BufReader::new(child.stdout.take().unwrap())
            .lines()
            .map_while(Result::ok)
        {
            emit(line);
        }
        let _ = err_thread.join();
        let status = child.wait()?;
        anyhow::ensure!(
            status.success(),
            "mise install failed (exit {:?})",
            status.code()
        );
        Ok(())
    })
    .await
}

/// `git init` plus a first commit, so changes have history from day one.
#[tauri::command]
pub async fn setup_git_init(state: State<'_, AppState>) -> CmdResult<String> {
    let v = state.vault()?;
    blocking(move || {
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&v)
                .stdin(Stdio::null())
                .output()
        };
        let init = git(&["init"])?;
        anyhow::ensure!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        git(&["add", "-A"])?;
        let commit = git(&["commit", "-m", "Start the vault"])?;
        Ok(if commit.status.success() {
            "Tracking the vault with git.".into()
        } else {
            format!(
                "Initialized git, but the first commit failed: {}",
                String::from_utf8_lossy(&commit.stderr).trim()
            )
        })
    })
    .await
}

/// Hides (or shows again) folders from every agent.
#[tauri::command]
pub async fn setup_hide_folders(
    state: State<'_, AppState>,
    folders: Vec<String>,
    hidden: bool,
) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || {
        for f in folders {
            VaultConfig::set_folder_agent(&v, &f, hidden.then_some("none"))?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn setup_complete(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let v: PathBuf = state.vault()?;
    let mut s = crate::settings::load(&app);
    if !s.setup_done.contains(&v) {
        s.setup_done.push(v);
    }
    crate::settings::save(&app, &s)
}
