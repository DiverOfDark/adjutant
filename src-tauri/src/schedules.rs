//! Built-in scheduler. Deterministic jobs (Jira sync, task archive) run
//! in-process; agent jobs open their own ACP session, send the stored prompt
//! and write the reply to the output file.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use adjutant_acp::{AgentEvent, AgentHandle, PermissionPolicy, PolicyDecision, PromptPart};
use adjutant_core::config::{JobConfig, OutputMode, ScheduleConfig, VaultConfig};
use adjutant_core::scheduler::{self, RunRecord, RunStatus, ScheduleView, SchedulerState, Trigger};
use adjutant_core::tasks;
use chrono::{Local, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::state::{AppState, CmdResult, blocking};

const TICK: Duration = Duration::from_secs(30);
/// Agent jobs that take longer than this are cancelled.
const AGENT_JOB_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// Serializes access to the scheduler state file.
static STATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunEvent {
    id: String,
    record: RunRecord,
}

/// Starts the scheduler loop. Runs for the app's lifetime.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut first = true;
        loop {
            if let Some(vault) = app.state::<AppState>().vault_opt() {
                if first {
                    let _guard = STATE_LOCK.lock().unwrap();
                    if let Ok(mut st) = SchedulerState::load(&vault) {
                        st.mark_interrupted(Utc::now());
                        let _ = st.save(&vault);
                    }
                    first = false;
                }
                if let Err(e) = tick(&app, &vault) {
                    tracing::warn!("scheduler tick failed: {e:#}");
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

fn tick(app: &AppHandle, vault: &Path) -> anyhow::Result<()> {
    let cfg = VaultConfig::load(vault)?;
    let schedules = scheduler::effective_schedules(&cfg);
    let due = {
        let _guard = STATE_LOCK.lock().unwrap();
        let mut st = SchedulerState::load(vault)?;
        let running = app.state::<AppState>().running_jobs.lock().unwrap().clone();
        let due: Vec<_> = scheduler::due_jobs(&schedules, &mut st, Utc::now())
            .into_iter()
            .filter(|d| !running.contains(&d.id))
            .collect();
        for d in &due {
            scheduler::mark_consumed(&mut st, &d.id, d.slot);
        }
        st.save(vault)?;
        due
    };
    for d in due {
        if let Some(s) = schedules.iter().find(|s| s.id == d.id).cloned() {
            // Jira sync is part of every new vault; stay quiet until Jira is set up.
            if matches!(s.job, JobConfig::JiraSync) && cfg.jira.is_none() {
                continue;
            }
            spawn_job(app.clone(), vault.to_path_buf(), s, d.trigger);
        }
    }
    Ok(())
}

fn spawn_job(app: AppHandle, vault: PathBuf, schedule: ScheduleConfig, trigger: Trigger) {
    {
        let state = app.state::<AppState>();
        let mut running = state.running_jobs.lock().unwrap();
        if !running.insert(schedule.id.clone()) {
            return;
        }
    }
    tauri::async_runtime::spawn(async move {
        let id = schedule.id.clone();
        let name = schedule.name.clone().unwrap_or_else(|| id.clone());
        update_state(&app, &vault, &id, |st| {
            st.record_start(&id, trigger, Utc::now())
        });

        let result = run_job(&app, &vault, &schedule).await;
        let (status, message, output, log) = match result {
            Ok(r) => (RunStatus::Ok, Some(r.message), r.output, r.log),
            Err(e) => (RunStatus::Failed, Some(e.message), None, e.log),
        };
        update_state(&app, &vault, &id, |st| {
            st.record_finish(
                &id,
                status,
                message.clone(),
                output.clone(),
                &log,
                Utc::now(),
            )
        });
        app.state::<AppState>()
            .running_jobs
            .lock()
            .unwrap()
            .remove(&id);
        app.state::<AppState>().audit.record_app(
            "schedule",
            serde_json::json!({"id": id, "status": status, "message": message, "output": output}),
        );

        let is_agent = matches!(schedule.job, JobConfig::Agent { .. });
        if is_agent || status == RunStatus::Failed {
            let body = match (&status, &output) {
                (RunStatus::Ok, Some(o)) => format!("Wrote {o}"),
                _ => message.clone().unwrap_or_default(),
            };
            let title = if status == RunStatus::Ok {
                name.clone()
            } else {
                format!("{name} failed")
            };
            let _ = app.notification().builder().title(title).body(body).show();
        }
    });
}

fn update_state(app: &AppHandle, vault: &Path, id: &str, f: impl FnOnce(&mut SchedulerState)) {
    let record = {
        let _guard = STATE_LOCK.lock().unwrap();
        let Ok(mut st) = SchedulerState::load(vault) else {
            return;
        };
        f(&mut st);
        let _ = st.save(vault);
        st.jobs.get(id).and_then(|j| j.last_run.clone())
    };
    if let Some(record) = record {
        let _ = app.emit(
            "schedule-run",
            RunEvent {
                id: id.to_string(),
                record,
            },
        );
    }
}

struct JobOk {
    message: String,
    output: Option<String>,
    log: String,
}

struct JobErr {
    message: String,
    log: String,
}

impl From<String> for JobErr {
    fn from(message: String) -> Self {
        JobErr {
            log: message.clone(),
            message,
        }
    }
}

async fn run_job(
    app: &AppHandle,
    vault: &Path,
    schedule: &ScheduleConfig,
) -> Result<JobOk, JobErr> {
    match &schedule.job {
        JobConfig::JiraSync => {
            let cache = crate::jira::run_sync(app, vault).await?;
            let message = format!(
                "Synced {} issues in {} queries",
                cache.issues.len(),
                cache.queries.len()
            );
            Ok(JobOk {
                log: message.clone(),
                message,
                output: Some(adjutant_core::jira::SNAPSHOT_PATH.into()),
            })
        }
        JobConfig::TaskArchive => {
            let v = vault.to_path_buf();
            let n = blocking(move || tasks::archive_done(&v, Local::now().date_naive())).await?;
            let message = format!("Archived {n} done tasks");
            Ok(JobOk {
                log: message.clone(),
                message,
                output: None,
            })
        }
        JobConfig::Agent {
            agent,
            prompt,
            output,
            output_mode,
        } => {
            run_agent_job(
                vault,
                agent.clone(),
                prompt,
                output.as_deref(),
                *output_mode,
            )
            .await
        }
    }
}

/// Unattended policy: allow the tool kinds listed in `permissions.job_allow`
/// (plus "always allow" rules), reject everything else.
struct JobPolicy {
    vault: PathBuf,
}

impl PermissionPolicy for JobPolicy {
    fn decide(&self, agent_id: &str, tool_call: &serde_json::Value) -> PolicyDecision {
        let kind = tool_call["kind"].as_str().unwrap_or("other");
        match VaultConfig::load(&self.vault) {
            Ok(cfg)
                if cfg.permissions.job_allow.iter().any(|k| k == kind)
                    || cfg.is_always_allowed(agent_id, kind) =>
            {
                PolicyDecision::Allow
            }
            _ => PolicyDecision::Reject,
        }
    }
}

#[derive(Default)]
struct Transcript {
    text: String,
    log: Vec<String>,
}

async fn run_agent_job(
    vault: &Path,
    agent: Option<String>,
    prompt: &str,
    output: Option<&str>,
    mode: OutputMode,
) -> Result<JobOk, JobErr> {
    let (launch, sandbox) = crate::agent::prepare_launch(vault, agent).await?;
    let agent_id = launch.id.clone();
    let transcript = Arc::new(Mutex::new(Transcript::default()));
    let t = transcript.clone();
    let sink = Arc::new(move |e: AgentEvent| {
        let mut t = t.lock().unwrap();
        match &e {
            AgentEvent::SessionUpdate { update, .. } => match update["sessionUpdate"].as_str() {
                Some("agent_message_chunk") => {
                    if let Some(s) = update["content"]["text"].as_str() {
                        t.text.push_str(s);
                    }
                }
                Some("tool_call") => t.log.push(format!(
                    "tool: {} ({})",
                    update["title"].as_str().unwrap_or("?"),
                    update["kind"].as_str().unwrap_or("other")
                )),
                _ => {}
            },
            AgentEvent::PermissionDecided { option_id, .. } => t.log.push(format!(
                "permission: {}",
                option_id.as_deref().unwrap_or("cancelled")
            )),
            AgentEvent::FileAccessDenied { path, reason, .. } => {
                t.log.push(format!("denied {}: {reason}", path.display()))
            }
            AgentEvent::Stderr { line } => t.log.push(format!("stderr: {line}")),
            AgentEvent::Error { message } => t.log.push(format!("error: {message}")),
            _ => {}
        }
    });
    let log_of = |t: &Arc<Mutex<Transcript>>| t.lock().unwrap().log.join("\n");

    let handle = AgentHandle::start(
        launch,
        sandbox,
        Arc::new(JobPolicy {
            vault: vault.to_path_buf(),
        }),
        sink,
    )
    .await
    .map_err(|e| JobErr {
        message: format!("{agent_id} did not start: {e:#}"),
        log: log_of(&transcript),
    })?;

    let full_prompt = format!(
        "{prompt}\n\n(This is an unattended scheduled job. Do not modify files. \
         Reply with the final Markdown only; Adjutant saves your reply{}.)",
        output.map(|o| format!(" to {o}")).unwrap_or_default()
    );
    let result = async {
        let session = handle.new_session(crate::agent::mcp_servers(vault)).await?;
        tokio::time::timeout(
            AGENT_JOB_TIMEOUT,
            handle.prompt(&session, vec![PromptPart::Text { text: full_prompt }]),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "timed out after {} minutes",
                AGENT_JOB_TIMEOUT.as_secs() / 60
            )
        })?
    }
    .await;
    handle.stop();

    let stop = result.map_err(|e| JobErr {
        message: format!("{e:#}"),
        log: log_of(&transcript),
    })?;
    let text = transcript.lock().unwrap().text.trim().to_string();
    let log = log_of(&transcript);
    if text.is_empty() {
        return Err(JobErr {
            message: format!("the agent returned no text (stop reason {stop})"),
            log,
        });
    }
    let Some(pattern) = output else {
        return Ok(JobOk {
            message: "Finished".into(),
            output: None,
            log: format!("{log}\n\n{text}"),
        });
    };
    let now = Local::now();
    let rel = scheduler::render_output_path(pattern, now);
    let v = vault.to_path_buf();
    let (rel2, text2) = (rel.clone(), text.clone());
    blocking(move || scheduler::write_output(&v, &rel2, &text2, mode, now))
        .await
        .map_err(|e| JobErr {
            message: e,
            log: log.clone(),
        })?;
    Ok(JobOk {
        message: match mode {
            OutputMode::Append => format!("Appended to {rel}"),
            OutputMode::Replace => format!("Wrote {rel}"),
        },
        output: Some(rel),
        log,
    })
}

// ----- Commands -----

#[tauri::command]
pub async fn schedules_list(state: State<'_, AppState>) -> CmdResult<Vec<ScheduleView>> {
    let v = state.vault()?;
    blocking(move || {
        let cfg = VaultConfig::load(&v)?;
        let st = SchedulerState::load(&v)?;
        Ok(scheduler::views(
            &scheduler::effective_schedules(&cfg),
            &st,
            Utc::now(),
        ))
    })
    .await
}

#[tauri::command]
pub async fn schedule_toggle(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || scheduler::set_enabled(&v, &id, enabled)).await
}

#[tauri::command]
pub async fn schedule_update(
    state: State<'_, AppState>,
    id: String,
    cron: Option<String>,
    prompt: Option<String>,
    output: Option<String>,
) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || {
        scheduler::update_schedule(
            &v,
            &id,
            cron.as_deref(),
            prompt.as_deref(),
            output.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn schedule_run(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let v = state.vault()?;
    let cfg = VaultConfig::load(&v).map_err(crate::state::aerr)?;
    let schedule = scheduler::effective_schedules(&cfg)
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown schedule `{id}`"))?;
    spawn_job(app, v, schedule, Trigger::Manual);
    Ok(())
}
