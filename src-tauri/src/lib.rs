mod commands;
mod edit;
mod editor;
mod prompts;
mod sftp;
mod state;
mod updates;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Native file dialogs, driven by the backend only (see sftp.rs).
        .plugin(tauri_plugin_dialog::init())
        // Files dragged from the system onto the window (see sftp::on_drag_drop).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(drag) = event {
                sftp::on_drag_drop(window, drag);
            }
        })
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(AppState::new(&dir)?);
            app.manage(editor::EditorChoice::load(&dir));
            app.manage(updates::PendingUpdate::default());
            edit::remove_stale_copies();
            let sftp_state = sftp::SftpState::default();
            app.manage(sftp_state.edits());
            app.manage(sftp_state);
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
            updates::check_update,
            updates::install_update,
            sftp::sftp_open,
            sftp::sftp_close,
            sftp::sftp_list,
            sftp::sftp_mkdir,
            sftp::sftp_rename,
            sftp::sftp_remove,
            sftp::sftp_cancel,
            sftp::sftp_download,
            sftp::sftp_pick_upload,
            sftp::sftp_upload,
            sftp::sftp_forget_picks,
            sftp::sftp_sudo_password,
            sftp::sftp_sudo_cancel,
            edit::sftp_edit,
            edit::sftp_edit_resolve,
            editor::editor_get,
            editor::editor_pick,
            editor::editor_reset,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Termdock");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Transfers first (they need their connections to clean up).
            if let Some(sftp) = handle.try_state::<sftp::SftpState>() {
                let sftp = sftp.inner().clone();
                tauri::async_runtime::block_on(sftp.close_all());
            }
            close_sessions_and_edits(handle);
        }
    });
}

/// Closes every session and deletes the local copies of edited files.
pub(crate) fn close_sessions_and_edits(handle: &tauri::AppHandle) {
    if let Some(state) = handle.try_state::<AppState>() {
        state.close_all();
    }
    if let Some(edits) = handle.try_state::<std::sync::Arc<edit::EditState>>() {
        edits.clear();
    }
}
