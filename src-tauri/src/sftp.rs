//! IPC for the file browser. SFTP runs on the SSH connection of an open
//! terminal session (see `termdock_core::sftp`).
//!
//! Local paths never come from the UI: downloads are saved where the user
//! picks in a native save dialog, and uploads only accept tokens handed out
//! after the user picked files in a native open dialog. A compromised UI can
//! at most open those dialogs; it cannot read or write files on its own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};
use termdock_core::sftp::{self, CancelFlag, Listing, Sftp};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::commands::{CmdError, CmdResult};
use crate::state::AppState;

/// Upload tokens are single use and expire.
const PICK_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PICKED: usize = 1000;
const MAX_TITLE: usize = 100;
/// Progress messages are sent at most this often per transfer.
const PROGRESS_STEP: u64 = 256 * 1024;

struct Browser {
    session: u32,
    sftp: Arc<Sftp>,
}

struct Picked {
    path: PathBuf,
    at: Instant,
}

#[derive(Default)]
pub struct SftpState {
    next: AtomicU32,
    browsers: Mutex<HashMap<u32, Browser>>,
    transfers: Mutex<HashMap<u32, CancelFlag>>,
    picked: Mutex<HashMap<Uuid, Picked>>,
}

impl SftpState {
    fn get(&self, id: u32) -> CmdResult<Arc<Sftp>> {
        self.browsers
            .lock()
            .unwrap()
            .get(&id)
            .map(|b| b.sftp.clone())
            .ok_or_else(|| CmdError::msg("the file browser is closed"))
    }

    /// Drops the browsers of a terminal session that is closing.
    pub fn forget_session(&self, session: u32) {
        self.browsers.lock().unwrap().retain(|_, b| b.session != session);
    }

    fn start_transfer(&self) -> (u32, CancelFlag) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let flag = CancelFlag::default();
        self.transfers.lock().unwrap().insert(id, flag.clone());
        (id, flag)
    }

    fn end_transfer(&self, id: u32) {
        self.transfers.lock().unwrap().remove(&id);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    id: u32,
    home: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    transfer: u32,
    done: u64,
    total: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    token: Uuid,
    name: String,
    size: u64,
}

#[tauri::command]
pub async fn sftp_open(state: State<'_, AppState>, sftp: State<'_, SftpState>, session: u32) -> CmdResult<Opened> {
    let handle = state
        .sessions
        .lock()
        .unwrap()
        .get(&session)
        .cloned()
        .ok_or_else(|| CmdError::msg("the terminal session is closed"))?;
    let fs = Sftp::open(&handle).await?;
    let home = fs.canonicalize(".").await?;
    let id = sftp.next.fetch_add(1, Ordering::Relaxed) + 1;
    sftp.browsers.lock().unwrap().insert(
        id,
        Browser {
            session,
            sftp: Arc::new(fs),
        },
    );
    Ok(Opened { id, home })
}

#[tauri::command]
pub async fn sftp_close(sftp: State<'_, SftpState>, id: u32) -> CmdResult<()> {
    let removed = sftp.browsers.lock().unwrap().remove(&id);
    if let Some(b) = removed {
        b.sftp.close().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_list(sftp: State<'_, SftpState>, id: u32, path: String) -> CmdResult<Listing> {
    Ok(sftp.get(id)?.list(&path).await?)
}

#[tauri::command]
pub async fn sftp_mkdir(sftp: State<'_, SftpState>, id: u32, dir: String, name: String) -> CmdResult<()> {
    Ok(sftp.get(id)?.mkdir(&dir, &name).await?)
}

#[tauri::command]
pub async fn sftp_rename(sftp: State<'_, SftpState>, id: u32, dir: String, from: String, to: String) -> CmdResult<()> {
    Ok(sftp.get(id)?.rename(&dir, &from, &to).await?)
}

#[tauri::command]
pub async fn sftp_remove(sftp: State<'_, SftpState>, id: u32, dir: String, name: String) -> CmdResult<()> {
    Ok(sftp.get(id)?.remove(&dir, &name).await?)
}

#[tauri::command]
pub fn sftp_cancel(sftp: State<'_, SftpState>, transfer: u32) {
    if let Some(flag) = sftp.transfers.lock().unwrap().get(&transfer) {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Asks where to save `dir/name`, then downloads it. Returns the local path,
/// or `None` when the user cancelled the dialog.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    sftp: State<'_, SftpState>,
    id: u32,
    dir: String,
    name: String,
    title: String,
    on_progress: Channel<Progress>,
) -> CmdResult<Option<String>> {
    let fs = sftp.get(id)?;
    let remote = sftp::join(&dir, &name);
    let mut dialog = dialog(&app, &title).set_file_name(local_file_name(&name));
    if let Some(downloads) = directories::UserDirs::new().and_then(|d| d.download_dir().map(Path::to_path_buf)) {
        dialog = dialog.set_directory(downloads);
    }
    let (tx, rx) = oneshot::channel();
    dialog.save_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(local) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let local = local.into_path().map_err(|e| CmdError::msg(e.to_string()))?;

    let (transfer, cancel) = sftp.start_transfer();
    let mut reported = 0u64;
    let result = fs
        .download(&remote, &local, &cancel, |done, total| {
            if done - reported >= PROGRESS_STEP || Some(done) == total || reported == 0 {
                reported = done.max(1);
                let _ = on_progress.send(Progress { transfer, done, total });
            }
        })
        .await;
    sftp.end_transfer(transfer);
    result?;
    Ok(Some(local.display().to_string()))
}

/// Native multi-file picker for uploads. Returns one single-use token per
/// regular file; nothing is read yet.
#[tauri::command]
pub async fn sftp_pick_upload(app: AppHandle, sftp: State<'_, SftpState>, title: String) -> CmdResult<Vec<PickedFile>> {
    let (tx, rx) = oneshot::channel();
    dialog(&app, &title).pick_files(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(files) = rx.await.ok().flatten() else {
        return Ok(Vec::new());
    };
    let mut picked = sftp.picked.lock().unwrap();
    picked.retain(|_, p| p.at.elapsed() < PICK_TTL);
    let mut out = Vec::new();
    for file in files {
        let Ok(path) = file.into_path() else { continue };
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() || picked.len() >= MAX_PICKED {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let token = Uuid::new_v4();
        picked.insert(
            token,
            Picked {
                path,
                at: Instant::now(),
            },
        );
        out.push(PickedFile {
            token,
            name,
            size: meta.len(),
        });
    }
    Ok(out)
}

/// Uploads a file picked with `sftp_pick_upload` into `dir`, under its own
/// name. Refused if that name exists, unless `overwrite`.
#[tauri::command]
pub async fn sftp_upload(
    sftp: State<'_, SftpState>,
    id: u32,
    dir: String,
    token: Uuid,
    overwrite: bool,
    on_progress: Channel<Progress>,
) -> CmdResult<()> {
    let fs = sftp.get(id)?;
    let picked = sftp
        .picked
        .lock()
        .unwrap()
        .remove(&token)
        .filter(|p| p.at.elapsed() < PICK_TTL)
        .ok_or_else(|| CmdError::msg("pick the files again"))?;
    let name = picked
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (transfer, cancel) = sftp.start_transfer();
    let mut reported = 0u64;
    let result = fs
        .upload(&picked.path, &dir, &name, overwrite, &cancel, |done, total| {
            if done - reported >= PROGRESS_STEP || Some(done) == total || reported == 0 {
                reported = done.max(1);
                let _ = on_progress.send(Progress { transfer, done, total });
            }
        })
        .await;
    sftp.end_transfer(transfer);
    Ok(result.map(|_| ())?)
}

fn dialog(app: &AppHandle, title: &str) -> FileDialogBuilder<tauri::Wry> {
    let mut d = app
        .dialog()
        .file()
        .set_title(title.chars().take(MAX_TITLE).collect::<String>());
    if let Some(window) = app.get_webview_window("main") {
        d = d.set_parent(&window);
    }
    d
}

/// A safe default name for the save dialog: the server's name without path
/// separators, characters Windows rejects, or control characters.
fn local_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || is_bidi_control(c) || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).trim_start_matches('.');
    let stem = cleaned.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    match cleaned {
        "" => "download".to_owned(),
        c if reserved => format!("_{c}"),
        c => c.chars().take(200).collect(),
    }
}

/// Invisible characters that reorder text: "evil\u{202e}gpj.exe" displays as
/// "evilexe.jpg".
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::local_file_name;

    #[test]
    fn local_names() {
        assert_eq!(local_file_name("report.pdf"), "report.pdf");
        assert_eq!(local_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(local_file_name("a\\b:c*?.txt"), "a_b_c__.txt");
        assert_eq!(local_file_name("evil\u{202e}gpj.exe"), "evil_gpj.exe");
        assert_eq!(local_file_name("..."), "download");
        assert_eq!(local_file_name(".bashrc"), "bashrc");
        assert_eq!(local_file_name("con.txt"), "_con.txt");
        assert_eq!(local_file_name("trailing. "), "trailing");
    }
}
