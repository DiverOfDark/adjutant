mod agent;
mod audit;
mod desktop;
mod jira;
mod schedules;
mod settings;
mod setup;
mod state;
mod tasks;
mod vault;
mod watcher;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info,adjutant=debug".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `adjutant --capture` from a system shortcut toggles quick capture.
            if args.iter().any(|a| a == "--capture") {
                desktop::toggle_capture(app);
            } else {
                desktop::show_main(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(AppState::default())
        .setup(|app| {
            desktop::setup(app.handle())?;
            if std::env::args().any(|a| a == "--minimized" || a == "--capture")
                && let Some(w) = tauri::Manager::get_webview_window(app, "main")
            {
                let _ = w.hide();
            }
            schedules::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault::vault_open,
            vault::vault_last,
            vault::vault_scaffold,
            vault::vault_tree,
            vault::note_read,
            vault::note_write,
            vault::note_create,
            vault::note_rename,
            vault::note_trash,
            vault::dir_create,
            vault::notes_list,
            vault::backlinks,
            vault::search,
            vault::templates_list,
            vault::repos_list,
            vault::note_visibility,
            vault::sessions_list,
            vault::session_save,
            vault::session_load,
            vault::repo_pull,
            tasks::tasks_list,
            tasks::task_complete,
            tasks::task_reopen,
            tasks::task_undo_complete,
            tasks::task_add,
            tasks::task_move,
            tasks::capture_note,
            agent::agents_discover,
            agent::agents_trust_vault,
            agent::agent_connect,
            agent::agent_disconnect,
            agent::chat_new_session,
            agent::chat_prompt,
            agent::chat_cancel,
            agent::permission_respond,
            jira::jira_settings,
            jira::jira_save_settings,
            jira::jira_test,
            jira::jira_sync,
            jira::jira_cache,
            schedules::schedules_list,
            schedules::schedule_toggle,
            schedules::schedule_update,
            schedules::schedule_run,
            settings::app_settings,
            settings::app_save_settings,
            desktop::capture_hide,
            setup::setup_status,
            setup::setup_install_tools,
            setup::setup_git_init,
            setup::setup_hide_folders,
            setup::setup_complete,
            agent::agent_authenticate,
            agent::agent_login_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
