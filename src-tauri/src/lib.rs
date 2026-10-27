mod agent;
mod audit;
mod secrets;
mod state;
mod vault;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info,adjutant=debug".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            vault::vault_open,
            vault::vault_last,
            agent::agents_discover,
            agent::agents_trust_vault,
            agent::agent_connect,
            agent::agent_disconnect,
            agent::chat_new_session,
            agent::chat_prompt,
            agent::chat_cancel,
            agent::permission_respond,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
