//! Pure scheduling logic. The app owns the timer and runs the jobs; this
//! module decides what is due, records runs in `.vault/cache/schedules.json`,
//! renders output paths, writes outputs and edits `[[schedules]]` in
//! `.vault/config.toml`.
//!
//! Cron expressions are local wall-clock time. Missed runs (laptop asleep,
//! app closed) are caught up once, never twice.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use anyhow::{Context, anyhow, bail};
use chrono::{DateTime, Datelike, Local, TimeZone, Utc};
use croner::Cron;
use croner::parser::{CronParser, Seconds, Year};
use serde::{Deserialize, Serialize};

use crate::config::{CONFIG_PATH, JobConfig, OutputMode, ScheduleConfig, VaultConfig};

pub const STATE_PATH: &str = ".vault/cache/schedules.json";

/// Id of the implicit Jira sync schedule added by [`effective_schedules`].
pub const JIRA_SYNC_ID: &str = "jira-sync";
const MAX_HISTORY: usize = 20;
const MAX_LOG_CHARS: usize = 20_000;
/// A slot this close to `now` counts as on time rather than a catch-up.
const ON_TIME_SECS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Schedule,
    CatchUp,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    /// Short summary or error.
    pub message: Option<String>,
    /// Vault-relative output file.
    pub output: Option<String>,
    /// Run log, truncated to the last 20k chars.
    pub log: String,
    pub trigger: Trigger,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub last_run: Option<RunRecord>,
    /// The scheduled slot last consumed (fired or caught up).
    pub last_scheduled: Option<DateTime<Utc>>,
    pub first_seen: DateTime<Utc>,
    /// Finished runs, newest first, at most 20.
    pub history: Vec<RunRecord>,
}

impl JobState {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            last_run: None,
            last_scheduled: None,
            first_seen: now,
            history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SchedulerState {
    pub jobs: BTreeMap<String, JobState>,
}

impl SchedulerState {
    /// Loads the state; a missing or corrupt file yields the default.
    pub fn load(vault: &Path) -> anyhow::Result<Self> {
        let path = vault.join(STATE_PATH);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("ignoring corrupt {}: {e}", path.display());
                Self::default()
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, vault: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        atomic_write(&vault.join(STATE_PATH), json.as_bytes())
    }

    /// Marks a job as running.
    pub fn record_start(&mut self, id: &str, trigger: Trigger, now: DateTime<Utc>) {
        let job = self
            .jobs
            .entry(id.to_string())
            .or_insert_with(|| JobState::new(now));
        job.last_run = Some(RunRecord {
            started_at: now,
            finished_at: None,
            status: RunStatus::Running,
            message: None,
            output: None,
            log: String::new(),
            trigger,
        });
    }

    /// Completes the running record (or creates one) and adds it to history.
    pub fn record_finish(
        &mut self,
        id: &str,
        status: RunStatus,
        message: Option<String>,
        output: Option<String>,
        log: &str,
        now: DateTime<Utc>,
    ) {
        let job = self
            .jobs
            .entry(id.to_string())
            .or_insert_with(|| JobState::new(now));
        let mut run = match job.last_run.take() {
            Some(r) if r.status == RunStatus::Running => r,
            _ => RunRecord {
                started_at: now,
                finished_at: None,
                status: RunStatus::Running,
                message: None,
                output: None,
                log: String::new(),
                trigger: Trigger::Manual,
            },
        };
        run.finished_at = Some(now);
        run.status = status;
        run.message = message;
        run.output = output;
        run.log = truncate_log(log);
        job.history.insert(0, run.clone());
        job.history.truncate(MAX_HISTORY);
        job.last_run = Some(run);
    }

    /// Call once at app start: runs left `Running` by a crash or quit are
    /// marked failed so they don't block their schedule forever.
    pub fn mark_interrupted(&mut self, now: DateTime<Utc>) {
        let ids: Vec<String> = self
            .jobs
            .iter()
            .filter(|(_, j)| {
                j.last_run
                    .as_ref()
                    .is_some_and(|r| r.status == RunStatus::Running)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let log = self.jobs[&id]
                .last_run
                .as_ref()
                .map(|r| r.log.clone())
                .unwrap_or_default();
            self.record_finish(
                &id,
                RunStatus::Failed,
                Some("interrupted (app closed while running)".into()),
                None,
                &log,
                now,
            );
        }
    }
}

/// Keeps the last 20k chars, prefixed with a truncation marker.
fn truncate_log(log: &str) -> String {
    let count = log.chars().count();
    if count <= MAX_LOG_CHARS {
        return log.to_string();
    }
    let tail: String = log.chars().skip(count - MAX_LOG_CHARS).collect();
    format!("…(truncated)\n{tail}")
}

fn parse_cron(cron: &str) -> anyhow::Result<Cron> {
    CronParser::builder()
        .seconds(Seconds::Optional)
        .year(Year::Disallowed)
        .build()
        .parse(cron)
        .map_err(|e| anyhow!("invalid cron expression `{cron}`: {e}"))
}

pub fn validate_cron(cron: &str) -> anyhow::Result<()> {
    parse_cron(cron).map(|_| ())
}

/// Next fire time strictly after `after`, evaluated in the local timezone.
pub fn next_run(cron: &str, after: DateTime<Utc>) -> anyhow::Result<DateTime<Utc>> {
    next_run_in(cron, after, &Local)
}

fn next_run_in<Tz: TimeZone>(
    cron: &str,
    after: DateTime<Utc>,
    tz: &Tz,
) -> anyhow::Result<DateTime<Utc>> {
    let c = parse_cron(cron)?;
    c.find_next_occurrence(&after.with_timezone(tz), false)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| anyhow!("no upcoming time for `{cron}`: {e}"))
}

/// Latest slot `<= now`, if any.
fn latest_slot_in<Tz: TimeZone>(cron: &Cron, now: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
    cron.find_previous_occurrence(&now.with_timezone(tz), true)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DueJob {
    pub id: String,
    /// The slot being fired; pass it to [`mark_consumed`].
    pub slot: DateTime<Utc>,
    /// `Schedule` if the slot is within 2 minutes of now, else `CatchUp`.
    pub trigger: Trigger,
}

/// Which enabled schedules are due at `now` (local-time cron).
///
/// A job is due when a slot exists in `(max(last_scheduled, first_seen), now]`.
/// Only the latest such slot is reported, so consuming it skips all earlier
/// missed slots. Unknown jobs are registered with `first_seen = now` and never
/// fire for the past. Jobs whose last run is still `Running` are skipped.
pub fn due_jobs(
    schedules: &[ScheduleConfig],
    state: &mut SchedulerState,
    now: DateTime<Utc>,
) -> Vec<DueJob> {
    due_jobs_in(schedules, state, now, &Local)
}

fn due_jobs_in<Tz: TimeZone>(
    schedules: &[ScheduleConfig],
    state: &mut SchedulerState,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Vec<DueJob> {
    let mut due = Vec::new();
    for s in schedules {
        let job = state
            .jobs
            .entry(s.id.clone())
            .or_insert_with(|| JobState::new(now));
        let Ok(cron) = parse_cron(&s.cron) else {
            continue;
        };
        let latest = latest_slot_in(&cron, now, tz);
        if !s.enabled {
            // Consume slots while disabled so re-enabling never fires for the past.
            if let Some(slot) = latest {
                mark_slot(job, slot);
            }
            continue;
        }
        if job
            .last_run
            .as_ref()
            .is_some_and(|r| r.status == RunStatus::Running)
        {
            continue;
        }
        let anchor = job
            .last_scheduled
            .map_or(job.first_seen, |l| l.max(job.first_seen));
        let Some(slot) = latest else {
            continue;
        };
        if slot > anchor {
            let trigger = if (now - slot).num_seconds() <= ON_TIME_SECS {
                Trigger::Schedule
            } else {
                Trigger::CatchUp
            };
            due.push(DueJob {
                id: s.id.clone(),
                slot,
                trigger,
            });
        }
    }
    due
}

/// Records that `slot` has been handled (fired, caught up or skipped).
pub fn mark_consumed(state: &mut SchedulerState, id: &str, slot: DateTime<Utc>) {
    let job = state
        .jobs
        .entry(id.to_string())
        .or_insert_with(|| JobState::new(slot));
    mark_slot(job, slot);
}

fn mark_slot(job: &mut JobState, slot: DateTime<Utc>) {
    if job.last_scheduled.is_none_or(|l| l < slot) {
        job.last_scheduled = Some(slot);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleView {
    pub id: String,
    pub name: String,
    pub cron: String,
    /// `jira-sync` | `task-archive` | `agent`
    pub kind: String,
    pub enabled: bool,
    pub next_run: Option<DateTime<Utc>>,
    pub last_run: Option<RunRecord>,
    pub prompt: Option<String>,
    pub output: Option<String>,
    pub output_mode: Option<OutputMode>,
    pub agent: Option<String>,
    /// Set when the cron expression is invalid.
    pub error: Option<String>,
}

pub fn job_kind(job: &JobConfig) -> &'static str {
    match job {
        JobConfig::JiraSync => "jira-sync",
        JobConfig::TaskArchive => "task-archive",
        JobConfig::Agent { .. } => "agent",
    }
}

pub fn views(
    schedules: &[ScheduleConfig],
    state: &SchedulerState,
    now: DateTime<Utc>,
) -> Vec<ScheduleView> {
    schedules
        .iter()
        .map(|s| {
            let (next_run, error) = match next_run(&s.cron, now) {
                Ok(t) => (s.enabled.then_some(t), None),
                Err(e) => (None, Some(e.to_string())),
            };
            let (prompt, output, output_mode, agent) = match &s.job {
                JobConfig::Agent {
                    agent,
                    prompt,
                    output,
                    output_mode,
                } => (
                    Some(prompt.clone()),
                    output.clone(),
                    Some(*output_mode),
                    agent.clone(),
                ),
                _ => (None, None, None, None),
            };
            ScheduleView {
                id: s.id.clone(),
                name: s.name.clone().unwrap_or_else(|| default_name(s)),
                cron: s.cron.clone(),
                kind: job_kind(&s.job).to_string(),
                enabled: s.enabled,
                next_run,
                last_run: state.jobs.get(&s.id).and_then(|j| j.last_run.clone()),
                prompt,
                output,
                output_mode,
                agent,
                error,
            }
        })
        .collect()
}

fn default_name(s: &ScheduleConfig) -> String {
    match s.job {
        JobConfig::JiraSync => "Jira sync".into(),
        JobConfig::TaskArchive => "Task archive".into(),
        JobConfig::Agent { .. } => s.id.clone(),
    }
}

/// Configured schedules plus an implicit `jira-sync` job when `[jira]` is
/// set and no schedule has kind `jira-sync`.
pub fn effective_schedules(cfg: &VaultConfig) -> Vec<ScheduleConfig> {
    let mut out = cfg.schedules.clone();
    if let Some(jira) = &cfg.jira
        && !out.iter().any(|s| matches!(s.job, JobConfig::JiraSync))
        && !out.iter().any(|s| s.id == JIRA_SYNC_ID)
    {
        out.push(implicit_jira_schedule(jira.sync_minutes.unwrap_or(15)));
    }
    out
}

fn implicit_jira_schedule(minutes: u64) -> ScheduleConfig {
    ScheduleConfig {
        id: JIRA_SYNC_ID.into(),
        name: Some("Jira sync".into()),
        cron: every_minutes_cron(minutes),
        job: JobConfig::JiraSync,
        enabled: true,
    }
}

/// Cron for "every `minutes` minutes" (approximated to whole hours above 59).
fn every_minutes_cron(minutes: u64) -> String {
    match minutes {
        0 | 1 => "* * * * *".into(),
        2..=59 => format!("*/{minutes} * * * *"),
        _ => {
            let hours = (minutes / 60).clamp(1, 24);
            if hours >= 24 {
                "0 0 * * *".into()
            } else if hours == 1 {
                "0 * * * *".into()
            } else {
                format!("0 */{hours} * * *")
            }
        }
    }
}

/// Expands `{date}` (2026-10-27), `{week}` (ISO, 2026-W44), `{month}`
/// (2026-10) and `{year}` (2026).
pub fn render_output_path(pattern: &str, now: DateTime<Local>) -> String {
    let w = now.iso_week();
    pattern
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{week}", &format!("{:04}-W{:02}", w.year(), w.week()))
        .replace("{month}", &now.format("%Y-%m").to_string())
        .replace("{year}", &now.format("%Y").to_string())
}

/// Writes a job's output. `Replace` writes the file atomically; `Append`
/// adds a `## <date>` section (creating the file with a `# <stem>` header).
pub fn write_output(
    vault: &Path,
    rel: &str,
    content: &str,
    mode: OutputMode,
    now: DateTime<Local>,
) -> anyhow::Result<()> {
    check_relative(rel)?;
    let path = vault.join(rel);
    let text = match mode {
        OutputMode::Replace => content.to_string(),
        OutputMode::Append => {
            let mut existing = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let stem = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    format!("# {stem}\n")
                }
                Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            };
            existing.push_str(&format!(
                "\n## {}\n\n{}\n",
                now.format("%Y-%m-%d"),
                content.trim_end()
            ));
            existing
        }
    };
    atomic_write(&path, text.as_bytes())
}

fn check_relative(rel: &str) -> anyhow::Result<()> {
    let p = Path::new(rel);
    if rel.trim().is_empty() {
        bail!("output path is empty");
    }
    if p.is_absolute() || rel.starts_with('/') || rel.starts_with('\\') {
        bail!("output path must be relative to the vault: {rel}");
    }
    for c in p.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => bail!("output path must stay inside the vault: {rel}"),
        }
    }
    if rel.split(['/', '\\']).any(|s| s == "..") {
        bail!("output path must stay inside the vault: {rel}");
    }
    Ok(())
}

/// Writes via a temp file in the same directory, then renames.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("no parent directory for {}", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        anyhow!("replacing {}: {e}", path.display())
    })
}

// ---- config editing ------------------------------------------------------

fn load_doc(vault: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let path = vault.join(CONFIG_PATH);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    text.parse().map_err(|e| anyhow!("{}: {e}", path.display()))
}

fn save_doc(vault: &Path, doc: &toml_edit::DocumentMut) -> anyhow::Result<()> {
    atomic_write(&vault.join(CONFIG_PATH), doc.to_string().as_bytes())
}

fn schedules_mut(
    doc: &mut toml_edit::DocumentMut,
) -> anyhow::Result<&mut toml_edit::ArrayOfTables> {
    doc.entry("schedules")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(Default::default()))
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("`schedules` in config.toml must be written as [[schedules]]"))
}

fn find_index(arr: &toml_edit::ArrayOfTables, id: &str) -> Option<usize> {
    arr.iter()
        .position(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))
}

fn schedule_table(s: &ScheduleConfig) -> anyhow::Result<toml_edit::Table> {
    let text = toml::to_string(s).context("serializing schedule")?;
    let doc: toml_edit::DocumentMut = text.parse()?;
    let mut table = doc.as_table().clone();
    table.set_implicit(false);
    table.decor_mut().clear();
    Ok(table)
}

/// Index of `id` in `[[schedules]]`; the implicit `jira-sync` job is written
/// into the file first so it can be edited like any other.
fn find_or_materialize(
    vault: &Path,
    doc: &mut toml_edit::DocumentMut,
    id: &str,
) -> anyhow::Result<usize> {
    if let Some(i) = find_index(schedules_mut(doc)?, id) {
        return Ok(i);
    }
    if id == JIRA_SYNC_ID {
        let cfg = VaultConfig::load(vault)?;
        if let Some(s) = effective_schedules(&cfg).into_iter().find(|s| s.id == id) {
            let arr = schedules_mut(doc)?;
            arr.push(schedule_table(&s)?);
            return Ok(arr.len() - 1);
        }
    }
    bail!("no schedule with id `{id}`")
}

/// Sets `key`, keeping any comment/whitespace around the old value.
fn set_keep_decor(t: &mut toml_edit::Table, key: &str, v: impl Into<toml_edit::Value>) {
    let mut v: toml_edit::Value = v.into();
    if let Some(old) = t.get(key).and_then(|i| i.as_value()) {
        *v.decor_mut() = old.decor().clone();
    }
    t[key] = toml_edit::Item::Value(v);
}

pub fn set_enabled(vault: &Path, id: &str, enabled: bool) -> anyhow::Result<()> {
    let mut doc = load_doc(vault)?;
    let i = find_or_materialize(vault, &mut doc, id)?;
    let t = schedules_mut(&mut doc)?
        .get_mut(i)
        .ok_or_else(|| anyhow!("schedule index out of range"))?;
    set_keep_decor(t, "enabled", enabled);
    save_doc(vault, &doc)
}

/// Updates the cron and, for agent jobs, the prompt and output pattern.
/// An empty `output` removes the output file.
pub fn update_schedule(
    vault: &Path,
    id: &str,
    cron: Option<&str>,
    prompt: Option<&str>,
    output: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(c) = cron {
        validate_cron(c)?;
    }
    if let Some(o) = output.filter(|o| !o.trim().is_empty()) {
        check_relative(o)?;
    }
    let mut doc = load_doc(vault)?;
    let i = find_or_materialize(vault, &mut doc, id)?;
    let t = schedules_mut(&mut doc)?
        .get_mut(i)
        .ok_or_else(|| anyhow!("schedule index out of range"))?;
    let is_agent = t.get("kind").and_then(|v| v.as_str()) == Some("agent");
    if (prompt.is_some() || output.is_some()) && !is_agent {
        bail!("only agent schedules have a prompt and output");
    }
    if let Some(c) = cron {
        set_keep_decor(t, "cron", c.trim());
    }
    if let Some(p) = prompt {
        if p.trim().is_empty() {
            bail!("prompt must not be empty");
        }
        set_keep_decor(t, "prompt", p);
    }
    match output.map(str::trim) {
        Some("") => {
            t.remove("output");
        }
        Some(o) => set_keep_decor(t, "output", o),
        None => {}
    }
    save_doc(vault, &doc)
}

pub fn add_schedule(vault: &Path, schedule: &ScheduleConfig) -> anyhow::Result<()> {
    if schedule.id.trim().is_empty() {
        bail!("schedule id must not be empty");
    }
    validate_cron(&schedule.cron)?;
    if let JobConfig::Agent {
        prompt,
        output: Some(o),
        ..
    } = &schedule.job
    {
        if prompt.trim().is_empty() {
            bail!("prompt must not be empty");
        }
        check_relative(o)?;
    }
    let mut doc = load_doc(vault)?;
    let arr = schedules_mut(&mut doc)?;
    if find_index(arr, &schedule.id).is_some() {
        bail!("a schedule with id `{}` already exists", schedule.id);
    }
    arr.push(schedule_table(schedule)?);
    save_doc(vault, &doc)
}

pub fn remove_schedule(vault: &Path, id: &str) -> anyhow::Result<()> {
    let mut doc = load_doc(vault)?;
    let arr = schedules_mut(&mut doc)?;
    let Some(i) = find_index(arr, id) else {
        if id == JIRA_SYNC_ID {
            bail!(
                "the Jira sync schedule is built in while [jira] is configured; disable it instead"
            );
        }
        bail!("no schedule with id `{id}`");
    };
    arr.remove(i);
    if arr.is_empty() {
        doc.remove("schedules");
    }
    save_doc(vault, &doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn sched(id: &str, cron: &str) -> ScheduleConfig {
        ScheduleConfig {
            id: id.into(),
            name: None,
            cron: cron.into(),
            job: JobConfig::TaskArchive,
            enabled: true,
        }
    }

    // ---- cron ----

    #[test]
    fn cron_five_and_six_fields() {
        // 2026-10-27 is a Tuesday.
        let t = utc(2026, 10, 27, 10, 0);
        assert_eq!(
            next_run_in("30 9 * * *", t, &Utc).unwrap(),
            utc(2026, 10, 28, 9, 30)
        );
        assert_eq!(
            next_run_in("*/15 * * * *", t, &Utc).unwrap(),
            utc(2026, 10, 27, 10, 15)
        );
        assert_eq!(
            next_run_in("30 0 16 * * *", t, &Utc).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 27, 16, 0, 30).unwrap()
        );
        assert_eq!(
            next_run_in("0 */15 * * * *", t, &Utc).unwrap(),
            utc(2026, 10, 27, 10, 15)
        );
        // strictly after
        assert_eq!(
            next_run_in("0 10 * * *", t, &Utc).unwrap(),
            utc(2026, 10, 28, 10, 0)
        );
    }

    #[test]
    fn cron_day_names() {
        let t = utc(2026, 10, 27, 10, 0); // Tuesday
        assert_eq!(
            next_run_in("0 17 * * FRI", t, &Utc).unwrap(),
            utc(2026, 10, 30, 17, 0)
        );
        assert_eq!(
            next_run_in("0 0 17 * * fri", t, &Utc).unwrap(),
            utc(2026, 10, 30, 17, 0)
        );
        let fri_eve = utc(2026, 10, 30, 19, 0);
        assert_eq!(
            next_run_in("0 9 * * MON-FRI", fri_eve, &Utc).unwrap(),
            utc(2026, 11, 2, 9, 0)
        );
        assert_eq!(
            next_run_in("0 9 * * MON-FRI", t, &Utc).unwrap(),
            utc(2026, 10, 28, 9, 0)
        );
        assert_eq!(
            next_run_in("0 9 1 * *", t, &Utc).unwrap(),
            utc(2026, 11, 1, 9, 0)
        );
    }

    #[test]
    fn cron_invalid() {
        for bad in [
            "",
            "nope",
            "* * *",
            "61 * * * *",
            "0 0 0 0 0 0 0",
            "0 9 * * FUNDAY",
        ] {
            assert!(validate_cron(bad).is_err(), "{bad:?} should be invalid");
        }
        assert!(validate_cron("0 17 * * FRI").is_ok());
        assert!(next_run("bad", Utc::now()).is_err());
        // Local-time wrapper agrees with the local evaluation.
        let now = Utc::now();
        assert!(next_run("*/5 * * * *", now).unwrap() > now);
    }

    // ---- due_jobs ----

    #[test]
    fn new_job_does_not_fire_for_the_past() {
        let s = [sched("a", "*/15 * * * *")];
        let mut st = SchedulerState::default();
        let t0 = utc(2026, 10, 27, 10, 7);
        assert!(due_jobs_in(&s, &mut st, t0, &Utc).is_empty());
        assert_eq!(st.jobs["a"].first_seen, t0);
        // Still nothing before the first slot.
        assert!(due_jobs_in(&s, &mut st, utc(2026, 10, 27, 10, 14), &Utc).is_empty());
    }

    #[test]
    fn fires_once_per_slot() {
        let s = [sched("a", "*/15 * * * *")];
        let mut st = SchedulerState::default();
        due_jobs_in(&s, &mut st, utc(2026, 10, 27, 10, 7), &Utc);
        let now = Utc.with_ymd_and_hms(2026, 10, 27, 10, 15, 3).unwrap();
        let due = due_jobs_in(&s, &mut st, now, &Utc);
        assert_eq!(
            due,
            vec![DueJob {
                id: "a".into(),
                slot: utc(2026, 10, 27, 10, 15),
                trigger: Trigger::Schedule
            }]
        );
        mark_consumed(&mut st, "a", due[0].slot);
        assert!(due_jobs_in(&s, &mut st, now + Duration::seconds(30), &Utc).is_empty());
        assert_eq!(
            due_jobs_in(&s, &mut st, utc(2026, 10, 27, 10, 30), &Utc).len(),
            1
        );
    }

    #[test]
    fn catches_up_once_after_long_gap_with_latest_slot() {
        let s = [sched("a", "0 * * * *")];
        let mut st = SchedulerState::default();
        due_jobs_in(&s, &mut st, utc(2026, 10, 27, 8, 30), &Utc);
        // Laptop asleep for two days.
        let now = utc(2026, 10, 29, 14, 20);
        let due = due_jobs_in(&s, &mut st, now, &Utc);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].slot, utc(2026, 10, 29, 14, 0));
        assert_eq!(due[0].trigger, Trigger::CatchUp);
        mark_consumed(&mut st, "a", due[0].slot);
        assert!(due_jobs_in(&s, &mut st, now, &Utc).is_empty());
        assert!(due_jobs_in(&s, &mut st, utc(2026, 10, 29, 14, 59), &Utc).is_empty());
        // Older slot can't move last_scheduled backwards.
        mark_consumed(&mut st, "a", utc(2026, 10, 28, 1, 0));
        assert_eq!(st.jobs["a"].last_scheduled, Some(utc(2026, 10, 29, 14, 0)));
    }

    #[test]
    fn weekly_job_missed_while_closed_catches_up() {
        let s = [sched("weekly", "0 17 * * FRI")];
        let mut st = SchedulerState::default();
        due_jobs_in(&s, &mut st, utc(2026, 10, 26, 9, 0), &Utc);
        // App closed over Friday; opened Monday.
        let due = due_jobs_in(&s, &mut st, utc(2026, 11, 2, 9, 0), &Utc);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].slot, utc(2026, 10, 30, 17, 0));
        assert_eq!(due[0].trigger, Trigger::CatchUp);
    }

    #[test]
    fn disabled_running_and_invalid_are_skipped() {
        let mut disabled = sched("off", "* * * * *");
        disabled.enabled = false;
        let s = [disabled, sched("run", "* * * * *"), sched("bad", "nope")];
        let mut st = SchedulerState::default();
        let t0 = utc(2026, 10, 27, 10, 0);
        due_jobs_in(&s, &mut st, t0, &Utc);
        assert!(st.jobs.contains_key("off"));
        assert!(
            due_jobs_in(&s, &mut st, utc(2026, 10, 27, 11, 0), &Utc)
                .iter()
                .all(|d| d.id != "off")
        );
        st.record_start("run", Trigger::Schedule, t0);
        let t1 = utc(2026, 10, 27, 10, 5);
        assert!(due_jobs_in(&s, &mut st, t1, &Utc).is_empty());
        st.record_finish("run", RunStatus::Ok, None, None, "", t1);
        let due = due_jobs_in(&s, &mut st, t1, &Utc);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "run");
        // Re-enabling a disabled job does not fire for slots missed while off.
        let mut on = s.clone();
        on[0].enabled = true;
        assert!(
            due_jobs_in(&on, &mut st, utc(2026, 10, 27, 11, 0), &Utc)
                .iter()
                .all(|d| d.id != "off")
        );
    }

    // ---- state ----

    #[test]
    fn run_records_and_history() {
        let mut st = SchedulerState::default();
        let t = utc(2026, 10, 27, 10, 0);
        for i in 0..25 {
            st.record_start("a", Trigger::Manual, t);
            assert_eq!(
                st.jobs["a"].last_run.as_ref().unwrap().status,
                RunStatus::Running
            );
            st.record_finish(
                "a",
                RunStatus::Ok,
                Some(format!("run {i}")),
                Some("reports/x.md".into()),
                "log",
                t + Duration::minutes(1),
            );
        }
        let job = &st.jobs["a"];
        assert_eq!(job.history.len(), 20);
        assert_eq!(job.history[0].message.as_deref(), Some("run 24"));
        let last = job.last_run.as_ref().unwrap();
        assert_eq!(last.status, RunStatus::Ok);
        assert_eq!(last.trigger, Trigger::Manual);
        assert_eq!(last.finished_at, Some(t + Duration::minutes(1)));

        let big = "x".repeat(30_000);
        st.record_finish("b", RunStatus::Failed, None, None, &big, t);
        let log = &st.jobs["b"].last_run.as_ref().unwrap().log;
        assert!(log.starts_with("…(truncated)"));
        assert!(log.chars().count() < 20_100);

        st.record_start("c", Trigger::Schedule, t);
        st.mark_interrupted(t);
        assert_eq!(
            st.jobs["c"].last_run.as_ref().unwrap().status,
            RunStatus::Failed
        );
    }

    #[test]
    fn state_save_load_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            SchedulerState::load(dir.path()).unwrap(),
            SchedulerState::default()
        );
        let mut st = SchedulerState::default();
        st.record_start("a", Trigger::CatchUp, utc(2026, 10, 27, 10, 0));
        st.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(STATE_PATH)).unwrap();
        assert!(text.contains("\"lastRun\""));
        assert!(text.contains("\"catchup\""));
        assert!(text.contains("\"running\""));
        assert_eq!(SchedulerState::load(dir.path()).unwrap(), st);
        std::fs::write(dir.path().join(STATE_PATH), "{garbage").unwrap();
        assert_eq!(
            SchedulerState::load(dir.path()).unwrap(),
            SchedulerState::default()
        );
    }

    // ---- output ----

    fn local(y: i32, m: u32, d: u32) -> DateTime<Local> {
        let naive = NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        Local.from_local_datetime(&naive).earliest().unwrap()
    }

    #[test]
    fn output_path_placeholders() {
        assert_eq!(
            render_output_path("reports/{week}.md", local(2026, 10, 27)),
            "reports/2026-W44.md"
        );
        assert_eq!(
            render_output_path("{year}/{month}/{date}.md", local(2026, 10, 27)),
            "2026/2026-10/2026-10-27.md"
        );
        // ISO week year boundaries.
        assert_eq!(render_output_path("{week}", local(2027, 1, 1)), "2026-W53");
        assert_eq!(
            render_output_path("{week}", local(2024, 12, 30)),
            "2025-W01"
        );
        assert_eq!(render_output_path("{week}", local(2021, 1, 3)), "2020-W53");
        assert_eq!(render_output_path("{week}", local(2026, 1, 5)), "2026-W02");
        assert_eq!(render_output_path("{week}", local(2026, 1, 1)), "2026-W01");
    }

    #[test]
    fn write_output_replace_and_append() {
        let dir = tempfile::tempdir().unwrap();
        let v = dir.path();
        write_output(
            v,
            "reports/w.md",
            "first",
            OutputMode::Replace,
            local(2026, 10, 27),
        )
        .unwrap();
        write_output(
            v,
            "reports/w.md",
            "second",
            OutputMode::Replace,
            local(2026, 10, 27),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(v.join("reports/w.md")).unwrap(),
            "second"
        );

        write_output(
            v,
            "logs/eod.md",
            "did A\n",
            OutputMode::Append,
            local(2026, 10, 27),
        )
        .unwrap();
        write_output(
            v,
            "logs/eod.md",
            "did B",
            OutputMode::Append,
            local(2026, 10, 28),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(v.join("logs/eod.md")).unwrap(),
            "# eod\n\n## 2026-10-27\n\ndid A\n\n## 2026-10-28\n\ndid B\n"
        );
        // No temp files left behind.
        assert_eq!(std::fs::read_dir(v.join("logs")).unwrap().count(), 1);
    }

    #[test]
    fn write_output_rejects_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let now = local(2026, 10, 27);
        for bad in ["../x.md", "a/../../x.md", "/etc/x.md", "", "a\\..\\x.md"] {
            assert!(
                write_output(dir.path(), bad, "x", OutputMode::Replace, now).is_err(),
                "{bad:?}"
            );
        }
        #[cfg(windows)]
        assert!(write_output(dir.path(), "C:\\x.md", "x", OutputMode::Replace, now).is_err());
    }

    // ---- effective schedules / views ----

    #[test]
    fn implicit_jira_sync() {
        let mut cfg = VaultConfig::default();
        assert!(effective_schedules(&cfg).is_empty());
        cfg.jira = Some(Default::default());
        let s = effective_schedules(&cfg);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, JIRA_SYNC_ID);
        assert_eq!(s[0].cron, "*/15 * * * *");
        assert!(s[0].enabled);
        cfg.jira.as_mut().unwrap().sync_minutes = Some(120);
        assert_eq!(effective_schedules(&cfg)[0].cron, "0 */2 * * *");
        let mut own = sched("my-jira", "0 * * * *");
        own.job = JobConfig::JiraSync;
        cfg.schedules.push(own);
        let s = effective_schedules(&cfg);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, "my-jira");
        for m in [0, 1, 7, 45, 60, 90, 600, 5000] {
            validate_cron(&every_minutes_cron(m)).unwrap();
        }
    }

    #[test]
    fn views_report_kind_next_and_errors() {
        let mut agent = sched("weekly", "0 17 * * FRI");
        agent.job = JobConfig::Agent {
            agent: Some("junie".into()),
            prompt: "Write it".into(),
            output: Some("reports/{week}.md".into()),
            output_mode: OutputMode::Replace,
        };
        let s = [agent, sched("bad", "nope")];
        let mut st = SchedulerState::default();
        let now = Utc::now();
        st.record_start("weekly", Trigger::Manual, now);
        let v = views(&s, &st, now);
        assert_eq!(v[0].kind, "agent");
        assert_eq!(v[0].prompt.as_deref(), Some("Write it"));
        assert_eq!(v[0].agent.as_deref(), Some("junie"));
        assert!(v[0].next_run.is_some());
        assert!(v[0].last_run.is_some());
        assert!(v[0].error.is_none());
        assert_eq!(v[1].kind, "task-archive");
        assert_eq!(v[1].name, "Task archive");
        assert!(v[1].next_run.is_none());
        assert!(v[1].error.is_some());
        let json = serde_json::to_value(&v[0]).unwrap();
        assert!(json.get("nextRun").is_some());
        assert_eq!(json["lastRun"]["trigger"], "manual");
    }

    // ---- config editing ----

    const CONFIG: &str = r#"# Adjutant config — keep this comment
[agent]
default = "junie"

[jira]
base_url = "https://acme.atlassian.net"
email = "me@example.com"

# Weekly report, every Friday
[[schedules]]
id = "weekly"
cron = "0 17 * * FRI" # local time
kind = "agent"
prompt = "Write the weekly report"
output = "reports/{week}.md"

[[schedules]]
id = "archive"
cron = "0 3 * * *"
kind = "task-archive"
"#;

    fn vault_with_config() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".vault")).unwrap();
        std::fs::write(dir.path().join(CONFIG_PATH), CONFIG).unwrap();
        dir
    }

    fn read_cfg(v: &Path) -> (String, VaultConfig) {
        (
            std::fs::read_to_string(v.join(CONFIG_PATH)).unwrap(),
            VaultConfig::load(v).unwrap(),
        )
    }

    #[test]
    fn edit_schedules_preserves_comments() {
        let dir = vault_with_config();
        let v = dir.path();

        set_enabled(v, "weekly", false).unwrap();
        update_schedule(v, "weekly", Some("0 16 * * FRI"), Some("New prompt"), None).unwrap();
        let (text, cfg) = read_cfg(v);
        assert!(text.contains("# Adjutant config — keep this comment"));
        assert!(text.contains("# Weekly report, every Friday"));
        assert!(text.contains("# local time"));
        assert!(!cfg.schedules[0].enabled);
        assert_eq!(cfg.schedules[0].cron, "0 16 * * FRI");
        assert!(
            matches!(&cfg.schedules[0].job, JobConfig::Agent { prompt, .. } if prompt == "New prompt")
        );

        // Validation.
        assert!(update_schedule(v, "weekly", Some("bogus"), None, None).is_err());
        assert!(update_schedule(v, "archive", None, Some("x"), None).is_err());
        assert!(update_schedule(v, "weekly", None, None, Some("../out.md")).is_err());
        assert!(set_enabled(v, "missing", true).is_err());
        update_schedule(v, "weekly", None, None, Some("")).unwrap();
        let (_, cfg) = read_cfg(v);
        assert!(matches!(
            &cfg.schedules[0].job,
            JobConfig::Agent { output: None, .. }
        ));

        // Add.
        let new = ScheduleConfig {
            id: "eod".into(),
            name: Some("End of day".into()),
            cron: "0 18 * * MON-FRI".into(),
            job: JobConfig::Agent {
                agent: None,
                prompt: "Line one\nLine \"two\"".into(),
                output: Some("logs/{month}.md".into()),
                output_mode: OutputMode::Append,
            },
            enabled: true,
        };
        add_schedule(v, &new).unwrap();
        assert!(add_schedule(v, &new).is_err());
        let mut bad = new.clone();
        bad.id = "bad".into();
        bad.cron = "nope".into();
        assert!(add_schedule(v, &bad).is_err());
        let (text, cfg) = read_cfg(v);
        assert!(text.contains("# Weekly report, every Friday"));
        assert_eq!(cfg.schedules.len(), 3);
        assert_eq!(cfg.schedules[2], new);

        // Remove.
        remove_schedule(v, "archive").unwrap();
        assert!(remove_schedule(v, "archive").is_err());
        let (text, cfg) = read_cfg(v);
        assert!(text.contains("# Adjutant config — keep this comment"));
        assert_eq!(
            cfg.schedules
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["weekly", "eod"]
        );
    }

    #[test]
    fn editing_implicit_jira_sync_materializes_it() {
        let dir = vault_with_config();
        let v = dir.path();
        assert!(remove_schedule(v, JIRA_SYNC_ID).is_err());
        set_enabled(v, JIRA_SYNC_ID, false).unwrap();
        let (text, cfg) = read_cfg(v);
        assert!(text.contains("# Weekly report, every Friday"));
        let s = cfg.schedules.iter().find(|s| s.id == JIRA_SYNC_ID).unwrap();
        assert!(!s.enabled);
        assert_eq!(s.job, JobConfig::JiraSync);
        assert_eq!(effective_schedules(&cfg).len(), 3);
    }

    #[test]
    fn add_to_missing_config() {
        let dir = tempfile::tempdir().unwrap();
        add_schedule(dir.path(), &sched("archive", "0 3 * * *")).unwrap();
        let (_, cfg) = read_cfg(dir.path());
        assert_eq!(cfg.schedules, vec![sched("archive", "0 3 * * *")]);
        remove_schedule(dir.path(), "archive").unwrap();
        assert!(VaultConfig::load(dir.path()).unwrap().schedules.is_empty());
    }
}
