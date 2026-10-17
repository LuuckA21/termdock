const COMMANDS: &[&str] = &[
    "app_info",
    "list_profiles",
    "save_profile",
    "delete_profile",
    "set_favorite",
    "reorder_favorites",
    "forget_secrets",
    "forget_host_key",
    "list_shells",
    "open_session",
    "write_session",
    "resize_session",
    "close_session",
    "answer_prompt",
    "preview_ssh_config",
    "import_ssh_config",
    "check_update",
    "install_update",
    "sftp_open",
    "sftp_close",
    "sftp_list",
    "sftp_mkdir",
    "sftp_rename",
    "sftp_remove",
    "sftp_cancel",
    "sftp_download",
    "sftp_pick_upload",
    "sftp_upload",
];

fn main() {
    // Generating per-command permissions means a command is only reachable
    // from the UI if a capability file explicitly allows it.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
