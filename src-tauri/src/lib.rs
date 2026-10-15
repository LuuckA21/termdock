mod commands;
mod prompts;
mod state;

use tauri::{Manager, RunEvent};
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("TERMDOCK_LOG").unwrap_or_else(|_| EnvFilter::new("info,russh=warn")))
        .init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(AppState::new(&dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::set_favorite,
            commands::reorder_favorites,
            commands::forget_secrets,
            commands::forget_host_key,
            commands::list_shells,
            commands::open_session,
            commands::write_session,
            commands::resize_session,
            commands::close_session,
            commands::answer_prompt,
            commands::preview_ssh_config,
            commands::import_ssh_config,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Termdock");

    app.run(|handle, event| {
        if let RunEvent::Exit = event
            && let Some(state) = handle.try_state::<AppState>()
        {
            state.close_all();
        }
    });
}
