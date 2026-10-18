//! Backup of the configuration: saved to a file the user picks, read back
//! from one, both in native dialogs (the UI never names a path).
//!
//! A backup holds the connections and favorites (no secrets: passwords live
//! in the OS keychain and are never exported, nor are keys or host keys) and
//! the interface's settings as the UI hands them over (appearance, language,
//! keys, layouts, snippets), which the UI validates again when importing.
//! Importing goes through a preview: nothing changes until the user picks
//! "add" or "replace", and the connections are checked as a whole first.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use termdock_core::profile::{Profile, ProfileKind};
use termdock_core::store::validate_backup;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::commands::{CmdError, CmdResult, blocking};
use crate::sftp::dialog;
use crate::state::AppState;

const FORMAT: &str = "termdock-backup";
const VERSION: u32 = 1;
const MAX_FILE: u64 = 16 * 1024 * 1024;
/// The interface's part, as JSON.
const MAX_UI: usize = 4 * 1024 * 1024;
/// A preview not acted on within this time is dropped.
const PREVIEW_TTL: Duration = Duration::from_secs(10 * 60);
/// Names of new connections shown in the preview.
const MAX_NAMES: usize = 10;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Backup {
    format: String,
    version: u32,
    /// Termdock version that wrote it.
    app: String,
    /// Seconds since the Unix epoch.
    exported: u64,
    profiles: Vec<Profile>,
    favorites: Vec<Uuid>,
    ui: serde_json::Value,
}

/// A backup read and checked, waiting for the user's choice.
#[derive(Default)]
pub struct PendingImport(Mutex<Option<(Uuid, Backup, Instant)>>);

/// What a backup holds, for the user to decide.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    token: Uuid,
    app: String,
    exported: u64,
    profiles: usize,
    /// Connections not in this Termdock yet, and a few of their names.
    new_profiles: usize,
    new_names: Vec<String>,
    favorites: usize,
    /// The interface's settings; the UI validates them before using them.
    ui: serde_json::Value,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Asks where to save, then writes the backup there. `ui`: the interface's
/// settings. Returns the path, or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn config_export(
    app: AppHandle,
    state: State<'_, AppState>,
    ui: serde_json::Value,
) -> CmdResult<Option<String>> {
    if serde_json::to_vec(&ui).map(|v| v.len()).unwrap_or(usize::MAX) > MAX_UI {
        return Err(CmdError::msg("the settings are too large to export"));
    }
    let (tx, rx) = oneshot::channel();
    let stamp = termdock_core::textlog::utc_stamp(now());
    dialog(&app)
        .set_file_name(format!("termdock-backup-{}.json", &stamp[..10]))
        .add_filter("JSON", &["json"])
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| CmdError::msg(e.to_string()))?;
    // Taken after the dialog: whatever was saved meanwhile is included.
    let backup = {
        let store = state.store.lock().unwrap();
        Backup {
            format: FORMAT.into(),
            version: VERSION,
            app: app.package_info().version.to_string(),
            exported: now(),
            profiles: store.profiles().to_vec(),
            favorites: store.favorites().to_vec(),
            ui,
        }
    };
    let target = path.clone();
    blocking(move || write_private(&target, &backup)).await??;
    Ok(Some(path.display().to_string()))
}

/// Written next to the target and moved into place when complete; private
/// to the owner on Unix (it lists hosts and user names).
fn write_private(path: &Path, backup: &Backup) -> CmdResult<()> {
    let dir = path
        .parent()
        .ok_or_else(|| CmdError::msg("choose a file in a folder"))?;
    let io = |e: std::io::Error| CmdError::from(termdock_core::Error::from(e));
    let mut tmp = tempfile::Builder::new()
        .prefix(".termdock-backup")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(io)?;
    serde_json::to_writer_pretty(tmp.as_file_mut(), backup).map_err(|e| CmdError::msg(e.to_string()))?;
    std::io::Write::write_all(tmp.as_file_mut(), b"\n").map_err(io)?;
    tmp.as_file_mut().sync_all().map_err(io)?;
    tmp.persist(path).map_err(|e| io(e.error))?;
    Ok(())
}

/// Asks for a backup file, reads and checks it, and returns what it holds.
/// `None` when the dialog was cancelled. Nothing is changed yet.
#[tauri::command]
pub async fn config_import_pick(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingImport>,
) -> CmdResult<Option<ImportPreview>> {
    let (tx, rx) = oneshot::channel();
    dialog(&app).add_filter("JSON", &["json"]).pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| CmdError::msg(e.to_string()))?;
    let mut backup = blocking(move || read_backup(&path)).await??;
    let (profiles, favorites) = validate_backup(backup.profiles, backup.favorites)?;
    backup.profiles = profiles;
    backup.favorites = favorites;

    let known: Vec<Uuid> = state.store.lock().unwrap().profiles().iter().map(|p| p.id).collect();
    let new: Vec<&Profile> = backup.profiles.iter().filter(|p| !known.contains(&p.id)).collect();
    let token = Uuid::new_v4();
    let preview = ImportPreview {
        token,
        app: backup.app.chars().take(32).collect(),
        exported: backup.exported,
        profiles: backup.profiles.len(),
        new_profiles: new.len(),
        new_names: new.iter().take(MAX_NAMES).map(|p| p.name.clone()).collect(),
        favorites: backup.favorites.len(),
        ui: backup.ui.clone(),
    };
    *pending.0.lock().unwrap() = Some((token, backup, Instant::now()));
    Ok(Some(preview))
}

fn read_backup(path: &Path) -> CmdResult<Backup> {
    let io = |e: std::io::Error| CmdError::from(termdock_core::Error::from(e));
    if std::fs::metadata(path).map_err(io)?.len() > MAX_FILE {
        return Err(CmdError::msg("this file is too large to be a Termdock backup"));
    }
    let bytes = std::fs::read(path).map_err(io)?;
    let backup: Backup =
        serde_json::from_slice(&bytes).map_err(|e| CmdError::msg(format!("not a Termdock backup: {e}")))?;
    if backup.format != FORMAT {
        return Err(CmdError::msg("not a Termdock backup"));
    }
    if backup.version > VERSION {
        return Err(CmdError::msg(
            "this backup was made by a newer Termdock: update this one first",
        ));
    }
    if serde_json::to_vec(&backup.ui).map(|v| v.len()).unwrap_or(usize::MAX) > MAX_UI {
        return Err(CmdError::msg("the settings in this backup are too large"));
    }
    Ok(backup)
}

/// Applies the backup previewed under `token`. `replace`: its connections
/// and favorites replace the current ones (saved passwords of connections
/// that go away or now point elsewhere are deleted first); otherwise only
/// connections not present yet are added. Returns how many were imported.
#[tauri::command(async)]
pub fn config_import_apply(
    state: State<'_, AppState>,
    pending: State<'_, PendingImport>,
    token: Uuid,
    replace: bool,
) -> CmdResult<usize> {
    let backup = match pending.0.lock().unwrap().take() {
        Some((t, backup, at)) if t == token && at.elapsed() < PREVIEW_TTL => backup,
        _ => return Err(CmdError::msg("open the backup again")),
    };
    let mut store = state.store.lock().unwrap();
    if !replace {
        return Ok(store.add_new(backup.profiles, backup.favorites)?);
    }
    // As when a connection is edited: a password saved for user@host must
    // not be offered to whatever the backup puts under that id.
    for old in store.profiles() {
        let ProfileKind::Ssh(was) = &old.kind else { continue };
        let kept = backup.profiles.iter().any(|p| {
            p.id == old.id
                && matches!(&p.kind, ProfileKind::Ssh(now)
                    if now.host == was.host && now.port == was.port && now.username == was.username && now.auth == was.auth)
        });
        if !kept {
            state.vault.delete_all(old.id)?;
        }
    }
    let count = backup.profiles.len();
    store.replace_all(backup.profiles, backup.favorites)?;
    Ok(count)
}

/// The user closed the preview without importing.
#[tauri::command]
pub fn config_import_cancel(pending: State<'_, PendingImport>) {
    pending.0.lock().unwrap().take();
}
