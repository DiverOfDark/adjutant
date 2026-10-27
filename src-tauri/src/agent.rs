//! Agent commands: discovery, connect, sessions, prompts, permissions.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use adjutant_acp::{
    AgentEvent, AgentHandle, AgentInfo, AgentLaunch, PermissionPolicy, PolicyDecision, PromptPart,
    Sandbox,
};
use adjutant_core::agents::{self, Discovery};
use adjutant_core::config::{PermissionRule, VaultConfig};
use adjutant_core::secrets;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, CmdResult, blocking, err};

/// Applies the vault's "always allow" rules; everything else asks the user.
struct ConfigPolicy {
    vault: PathBuf,
}

impl PermissionPolicy for ConfigPolicy {
    fn decide(&self, agent_id: &str, tool_call: &serde_json::Value) -> PolicyDecision {
        let kind = tool_call["kind"].as_str().unwrap_or("other");
        match VaultConfig::load(&self.vault) {
            Ok(cfg) if cfg.is_always_allowed(agent_id, kind) => PolicyDecision::Allow,
            _ => PolicyDecision::Ask,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentEventEnvelope {
    agent_id: String,
    #[serde(flatten)]
    event: AgentEvent,
}

#[tauri::command]
pub async fn agents_discover(state: State<'_, AppState>) -> CmdResult<Discovery> {
    let vault = state.vault()?;
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = VaultConfig::load(&vault).map_err(err)?;
        Ok(agents::discover(&vault, &cfg))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn agents_trust_vault(state: State<'_, AppState>) -> CmdResult<Discovery> {
    let vault = state.vault()?;
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = VaultConfig::load(&vault).map_err(err)?;
        let mise = agents::find_mise(cfg.agent.mise_bin.as_deref())
            .ok_or_else(|| "mise not found".to_string())?;
        agents::trust(&vault, &mise).map_err(err)?;
        Ok(agents::discover(&vault, &cfg))
    })
    .await
    .map_err(err)?
}

/// Resolves an agent id (or the vault default) into a launch spec and a
/// sandbox that hides `agent = "none"` folders. Shared with scheduled jobs.
pub async fn prepare_launch(
    vault: &Path,
    agent_id: Option<String>,
) -> CmdResult<(AgentLaunch, Sandbox)> {
    let v = vault.to_path_buf();
    blocking(move || {
        let cfg = VaultConfig::load(&v)?;
        let discovery = agents::discover(&v, &cfg);
        let id = agent_id
            .or(discovery.default_agent.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("no agent configured; add one to the vault's mise.toml")
            })?;
        let entry = discovery
            .agents
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown agent `{id}`"))?;
        if let Some(e) = &discovery.mise_error
            && entry.use_mise
        {
            anyhow::bail!("{e}");
        }
        anyhow::ensure!(
            entry.installed,
            "{} is not installed. Add this line to mise.toml: {}",
            entry.name,
            entry.mise_line.clone().unwrap_or_default()
        );
        let mut env = entry.env.clone();
        for (var, name) in &entry.secrets {
            if let Some(value) = secrets::get_secret(name)? {
                env.insert(var.clone(), value);
            }
        }
        let launch = AgentLaunch {
            id: entry.id.clone(),
            program: entry.command.clone(),
            args: entry.args.clone(),
            env,
            cwd: v.clone(),
            use_mise: entry.use_mise,
            mise_bin: discovery.mise_bin.clone(),
        };
        let sandbox = Sandbox::new([v.clone()])
            .with_denied(cfg.hidden_folders().into_iter().map(|f| v.join(f)));
        Ok((launch, sandbox))
    })
    .await
}

/// MCP servers from `.vault/config.toml`, with `env`/`headers` defaulted.
pub fn mcp_servers(vault: &Path) -> Vec<serde_json::Value> {
    VaultConfig::load(vault)
        .map(|c| c.mcp_servers)
        .unwrap_or_default()
        .into_iter()
        .map(|mut s| {
            if let Some(o) = s.as_object_mut() {
                if o.contains_key("url") {
                    o.entry("headers").or_insert(serde_json::json!([]));
                } else {
                    o.entry("args").or_insert(serde_json::json!([]));
                    o.entry("env").or_insert(serde_json::json!([]));
                }
            }
            s
        })
        .collect()
}

/// Starts `agent_id` (or the vault default), replacing any running agent.
#[tauri::command]
pub async fn agent_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_id: Option<String>,
) -> CmdResult<AgentInfo> {
    let vault = state.vault()?;
    let (launch, sandbox) = prepare_launch(&vault, agent_id).await?;

    if let Some(old) = state.agent.lock().await.take() {
        old.stop();
    }

    let agent_id = launch.id.clone();
    let audit = state.audit.clone();
    let sink = Arc::new(move |event: AgentEvent| {
        audit.record(&agent_id, &event);
        let _ = app.emit(
            "agent-event",
            AgentEventEnvelope {
                agent_id: agent_id.clone(),
                event,
            },
        );
    });
    let handle = AgentHandle::start(launch, sandbox, Arc::new(ConfigPolicy { vault }), sink)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let info = handle.info().clone();
    *state.agent.lock().await = Some(Arc::new(handle));
    Ok(info)
}

#[tauri::command]
pub async fn agent_disconnect(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(old) = state.agent.lock().await.take() {
        old.stop();
    }
    Ok(())
}

#[tauri::command]
pub async fn chat_new_session(state: State<'_, AppState>) -> CmdResult<String> {
    let agent = state.current_agent().await?;
    agent
        .new_session(vec![])
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Sends a prompt; resolves with the stop reason when the turn ends.
#[tauri::command]
pub async fn chat_prompt(
    state: State<'_, AppState>,
    session_id: String,
    parts: Vec<PromptPart>,
) -> CmdResult<serde_json::Value> {
    let agent = state.current_agent().await?;
    agent
        .prompt(&session_id, parts)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn chat_cancel(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    let agent = state.current_agent().await?;
    agent.cancel(&session_id).map_err(err)
}

/// Answers a permission prompt. With `always_tool_kind`, also stores an
/// "always allow" rule for this agent and tool kind in `.vault/config.toml`.
#[tauri::command]
pub async fn permission_respond(
    state: State<'_, AppState>,
    request_id: u64,
    option_id: Option<String>,
    always_tool_kind: Option<String>,
) -> CmdResult<()> {
    let agent = state.current_agent().await?;
    if let Some(tool) = always_tool_kind {
        let rule = PermissionRule {
            agent: agent.launch().id.clone(),
            tool,
        };
        VaultConfig::add_always_allow(&state.vault()?, &rule).map_err(err)?;
    }
    agent.respond_permission(request_id, option_id);
    Ok(())
}

/// ACP `authenticate` with one of the agent's auth methods ("agent" type:
/// the agent runs the login itself, e.g. opens a browser).
#[tauri::command]
pub async fn agent_authenticate(state: State<'_, AppState>, method_id: String) -> CmdResult<()> {
    let agent = state.current_agent().await?;
    agent
        .authenticate(&method_id)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Opens a system terminal in the vault running the agent's login: the
/// "terminal" auth method's args, or the bare agent command (an interactive
/// CLI that logs in on first run). Returns the command so the UI can show it.
#[tauri::command]
pub async fn agent_login_terminal(
    state: State<'_, AppState>,
    agent_id: Option<String>,
    method_id: Option<String>,
) -> CmdResult<String> {
    let vault = state.vault()?;
    let (launch, _) = prepare_launch(&vault, agent_id).await?;
    let mut args: Vec<String> = Vec::new();
    if let (Some(method), Some(agent)) = (&method_id, state.agent.lock().await.clone())
        && let Some(m) = agent.info().auth_methods.as_array().and_then(|ms| {
            ms.iter()
                .find(|m| m["id"].as_str() == Some(method.as_str()))
        })
    {
        args = m["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
    }
    let mut parts: Vec<String> = Vec::new();
    if launch.use_mise {
        let mise = launch
            .mise_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "mise".into());
        parts.extend([shell_quote(&mise), "exec".into(), "--".into()]);
    }
    parts.push(shell_quote(&launch.program));
    parts.extend(args.iter().map(|a| shell_quote(a)));
    let command = parts.join(" ");
    open_terminal(&vault, &command).map_err(|e| {
        format!(
            "{e}. Run this in a terminal in {}: {command}",
            vault.display()
        )
    })?;
    Ok(command)
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn open_terminal(cwd: &Path, command: &str) -> anyhow::Result<()> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\" to do script \"cd {} && {}\"",
            shell_quote(&cwd.display().to_string()).replace('"', "\\\""),
            command.replace('"', "\\\"")
        );
        Command::new("osascript")
            .args([
                "-e",
                &script,
                "-e",
                "tell application \"Terminal\" to activate",
            ])
            .spawn()?;
        return Ok(());
    }
    #[cfg(windows)]
    {
        Command::new("cmd")
            .args(["/C", "start", "Adjutant login", "cmd", "/K", command])
            .current_dir(cwd)
            .spawn()?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let script = format!("{command}; echo; read -r -p 'Done. Press Enter to close.' _");
        let mut candidates: Vec<(String, Vec<&str>)> = Vec::new();
        if let Ok(t) = std::env::var("TERMINAL") {
            candidates.push((t, vec!["-e"]));
        }
        for (bin, args) in [
            ("x-terminal-emulator", vec!["-e"]),
            ("gnome-terminal", vec!["--"]),
            ("kgx", vec!["--"]),
            ("ptyxis", vec!["--"]),
            ("konsole", vec!["-e"]),
            ("xfce4-terminal", vec!["-x"]),
            ("alacritty", vec!["-e"]),
            ("kitty", vec![]),
            ("wezterm", vec!["start", "--"]),
            ("foot", vec![]),
            ("xterm", vec!["-e"]),
        ] {
            candidates.push((bin.to_string(), args));
        }
        for (bin, args) in candidates {
            let ok = Command::new(&bin)
                .args(&args)
                .args(["bash", "-lc", &script])
                .current_dir(cwd)
                .spawn()
                .is_ok();
            if ok {
                return Ok(());
            }
        }
        anyhow::bail!("no terminal emulator found")
    }
}
