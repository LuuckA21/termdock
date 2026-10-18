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
    "sftp_pick_upload",
    "sftp_pick_upload_folders",
    "sftp_forget_picks",
    "sftp_plan_upload",
    "sftp_plan_download",
    "sftp_run_plan",
    "sftp_drop_plan",
    "sftp_sudo_password",
    "sftp_sudo_cancel",
    "sftp_edit",
    "sftp_edit_resolve",
    "editor_get",
    "editor_pick",
    "editor_reset",
];

fn main() {
    // Generating per-command permissions means a command is only reachable
    // from the UI if a capability file explicitly allows it.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
