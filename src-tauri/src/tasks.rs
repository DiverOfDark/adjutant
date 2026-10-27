//! Task commands. All deterministic: no agent involved.

use adjutant_core::tasks::{self, CompleteOutcome, CompleteUndo, Task};
use tauri::State;

use crate::state::{AppState, CmdResult, blocking};

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

#[tauri::command]
pub async fn tasks_list(state: State<'_, AppState>) -> CmdResult<Vec<Task>> {
    let v = state.vault()?;
    blocking(move || tasks::scan_vault(&v)).await
}

#[tauri::command]
pub async fn task_complete(
    state: State<'_, AppState>,
    file: String,
    line: usize,
    raw: String,
) -> CmdResult<CompleteOutcome> {
    let v = state.vault()?;
    let audit = state.audit.clone();
    let out = blocking(move || tasks::complete_task(&v, &file, line, &raw, today())).await?;
    audit.record_app(
        "task-complete",
        serde_json::to_value(&out).unwrap_or_default(),
    );
    Ok(out)
}

#[tauri::command]
pub async fn task_undo_complete(state: State<'_, AppState>, undo: CompleteUndo) -> CmdResult<()> {
    let v = state.vault()?;
    let audit = state.audit.clone();
    blocking(move || tasks::undo_complete(&v, &undo)).await?;
    audit.record_app("task-undo-complete", serde_json::Value::Null);
    Ok(())
}

#[tauri::command]
pub async fn task_reopen(
    state: State<'_, AppState>,
    file: String,
    line: usize,
    raw: String,
) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || tasks::reopen_task(&v, &file, line, &raw)).await
}

#[tauri::command]
pub async fn task_add(state: State<'_, AppState>, section: String, text: String) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || tasks::add_task(&v, &section, &text)).await
}

#[tauri::command]
pub async fn task_move(
    state: State<'_, AppState>,
    line: usize,
    raw: String,
    section: String,
) -> CmdResult<()> {
    let v = state.vault()?;
    blocking(move || tasks::move_task(&v, line, &raw, &section)).await
}

#[tauri::command]
pub async fn capture_note(state: State<'_, AppState>, text: String) -> CmdResult<String> {
    let v = state.vault()?;
    blocking(move || tasks::capture_note(&v, &text, chrono::Local::now().naive_local())).await
}
