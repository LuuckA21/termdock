//! In-app updates through tauri-plugin-updater.
//!
//! The UI can only ask whether a newer release exists and install the one the
//! last check found. Endpoint, public key and version policy are fixed in
//! `tauri.conf.json` (`plugins.updater`), so the UI cannot point the updater
//! elsewhere, skip the signature check or force a downgrade. The plugin's own
//! IPC commands are not allowed by any capability.
//!
//! Every package is verified against the embedded minisign public key, and the
//! signed version must match the announced one (`requireSignedVersion`), so a
//! tampered manifest cannot pass off an older signed release as a new one.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::utils::config::BundleType;
use tauri::utils::platform::bundle_type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::commands::{CmdError, CmdResult};
use crate::state::AppState;

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Release notes come from an unsigned manifest: cap what reaches the UI.
const MAX_NOTES_CHARS: usize = 8_000;
const PROGRESS_STEP: u64 = 256 * 1024;

/// The update found by the last check, waiting for the user's consent.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    notes: String,
    /// False when this copy cannot replace itself: deb/rpm packages belong to
    /// the system package manager, and dev builds have no package at all.
    installable: bool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum UpdateProgress {
    Downloading { downloaded: u64, total: Option<u64> },
    Installing,
}

/// Packages that update themselves in place. deb and rpm are left to the
/// package manager: installing them needs root, which the plugin would ask
/// for by piping the user's password to `sudo`.
fn installable() -> bool {
    !cfg!(debug_assertions)
        && matches!(
            bundle_type(),
            Some(BundleType::AppImage | BundleType::Msi | BundleType::Nsis | BundleType::App)
        )
}

fn close_sessions(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.close_all();
    }
}

#[tauri::command]
pub async fn check_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> CmdResult<Option<UpdateInfo>> {
    let handle = app.clone();
    let update = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        // Windows: the installer is started and the app exits right away.
        .on_before_exit(move || close_sessions(&handle))
        .build()?
        .check()
        .await?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u
            .body
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(MAX_NOTES_CHARS)
            .collect(),
        installable: installable(),
    });
    *pending.0.lock().unwrap() = update;
    Ok(info)
}

/// Downloads, verifies and installs the update found by `check_update`, then
/// restarts the app (on Windows the installer does it).
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    on_progress: Channel<UpdateProgress>,
) -> CmdResult<()> {
    if !installable() {
        return Err(CmdError::msg("this copy of Termdock cannot update itself"));
    }
    let Some(mut update) = pending.0.lock().unwrap().take() else {
        return Err(CmdError::msg("no update to install, check again"));
    };
    update.timeout = Some(DOWNLOAD_TIMEOUT);

    let mut downloaded = 0u64;
    let mut reported = 0u64;
    let result = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - reported >= PROGRESS_STEP || Some(downloaded) == total {
                    reported = downloaded;
                    let _ = on_progress.send(UpdateProgress::Downloading { downloaded, total });
                }
            },
            || {},
        )
        .await;
    // `download` verified the signature and the signed version.
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(e) => {
            // Keep it for another attempt.
            *pending.0.lock().unwrap() = Some(update);
            return Err(e.into());
        }
    };

    let _ = on_progress.send(UpdateProgress::Installing);
    // Verifies again, then replaces the app (Windows: starts the installer and exits).
    update.install(&bytes)?;
    close_sessions(&app);
    app.request_restart();
    Ok(())
}
