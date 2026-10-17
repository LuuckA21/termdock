//! IPC for the file browser. SFTP runs on the SSH connection of an open
//! terminal session (see `termdock_core::sftp`).
//!
//! Local paths never come from the UI: downloads are saved where the user
//! picks in a native save dialog, and uploads only accept tokens for files
//! the user picked in a native open dialog (opened right then, so the file
//! cannot be swapped later). The dialogs use the system's own titles: a
//! compromised UI cannot word them, and it can at most open them; it cannot
//! read or write files on its own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};
use termdock_core::sftp::{CancellationToken, Listing, Sftp, is_format_char};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::commands::{CmdError, CmdResult};
use crate::state::AppState;

/// Upload tokens are single use and expire.
const PICK_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PICKED: usize = 1000;
/// Progress messages are sent at most this often per transfer.
const PROGRESS_STEP: u64 = 256 * 1024;
/// How long closing waits for running transfers to clean up after cancel.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

struct Browser {
    session: u32,
    sftp: Arc<Sftp>,
}

struct Transfer {
    browser: u32,
    cancel: CancellationToken,
}

struct Picked {
    file: std::fs::File,
    mode: Option<u32>,
    name: String,
    at: Instant,
}

#[derive(Default)]
struct Inner {
    next: AtomicU32,
    browsers: Mutex<HashMap<u32, Browser>>,
    transfers: Mutex<HashMap<u32, Transfer>>,
    picked: Mutex<HashMap<Uuid, Picked>>,
}

/// Cheap to clone, so the session's exit handler can hold it.
#[derive(Clone, Default)]
pub struct SftpState(Arc<Inner>);

impl SftpState {
    fn get(&self, id: u32) -> CmdResult<Arc<Sftp>> {
        self.0
            .browsers
            .lock()
            .unwrap()
            .get(&id)
            .map(|b| b.sftp.clone())
            .ok_or_else(|| CmdError::msg("the connection of this file browser is closed"))
    }

    fn next_id(&self) -> u32 {
        self.0.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Removes browsers (all of `session`'s, or just `browser`) and cancels
    /// their transfers. Returns what to close and the transfers to wait for.
    fn detach(&self, session: Option<u32>, browser: Option<u32>) -> (Vec<Arc<Sftp>>, Vec<u32>) {
        let mut browsers = self.0.browsers.lock().unwrap();
        let ids: Vec<u32> = browsers
            .iter()
            .filter(|(id, b)| Some(b.session) == session || Some(**id) == browser)
            .map(|(id, _)| *id)
            .collect();
        let closed = ids
            .iter()
            .filter_map(|id| browsers.remove(id))
            .map(|b| b.sftp)
            .collect();
        drop(browsers);
        let transfers = self.0.transfers.lock().unwrap();
        let running = transfers
            .iter()
            .filter(|(_, t)| ids.contains(&t.browser))
            .map(|(id, t)| {
                t.cancel.cancel();
                *id
            })
            .collect();
        (closed, running)
    }

    /// Waits (bounded) for cancelled transfers to remove their temporary files.
    async fn drain(&self, running: &[u32]) {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while Instant::now() < deadline {
            if !self.0.transfers.lock().unwrap().keys().any(|id| running.contains(id)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The terminal session ended by itself: drop its browsers now.
    pub fn session_ended(&self, session: u32) {
        let (closed, _) = self.detach(Some(session), None);
        drop(closed);
    }

    /// The terminal session is being closed: cancel its transfers, let them
    /// clean up, then close the SFTP channels (before the SSH connection).
    pub async fn close_session(&self, session: u32) {
        let (closed, running) = self.detach(Some(session), None);
        self.drain(&running).await;
        for sftp in closed {
            sftp.close().await;
        }
    }

    fn start_transfer(&self, browser: u32, progress: &Channel<Progress>) -> (u32, CancellationToken) {
        let id = self.next_id();
        let cancel = CancellationToken::new();
        self.0.transfers.lock().unwrap().insert(
            id,
            Transfer {
                browser,
                cancel: cancel.clone(),
            },
        );
        // The UI learns the id at once, so Cancel works from the start.
        let _ = progress.send(Progress {
            transfer: id,
            done: 0,
            total: None,
        });
        (id, cancel)
    }

    fn end_transfer(&self, id: u32) {
        self.0.transfers.lock().unwrap().remove(&id);
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

fn throttled(transfer: u32, channel: Channel<Progress>) -> impl FnMut(u64, Option<u64>) {
    let mut reported = 0u64;
    move |done, total| {
        if done - reported >= PROGRESS_STEP || Some(done) == total {
            reported = done;
            let _ = channel.send(Progress { transfer, done, total });
        }
    }
}

#[tauri::command]
pub async fn sftp_open(state: State<'_, AppState>, sftp: State<'_, SftpState>, session: u32) -> CmdResult<Opened> {
    let live = |s: &State<'_, AppState>| s.sessions.lock().unwrap().get(&session).cloned();
    let handle = live(&state).ok_or_else(|| CmdError::msg("the terminal session is closed"))?;
    let fs = Sftp::open(&handle).await?;
    let home = fs.canonicalize(".").await?;
    let id = sftp.next_id();
    {
        // Registered only if the terminal is still open, so a browser can
        // never outlive its session.
        let mut browsers = sftp.0.browsers.lock().unwrap();
        if live(&state).is_some() {
            browsers.insert(
                id,
                Browser {
                    session,
                    sftp: Arc::new(fs),
                },
            );
            return Ok(Opened { id, home });
        }
    }
    fs.close().await;
    Err(CmdError::msg("the terminal session is closed"))
}

#[tauri::command]
pub async fn sftp_close(sftp: State<'_, SftpState>, id: u32) -> CmdResult<()> {
    let (closed, running) = sftp.detach(None, Some(id));
    sftp.drain(&running).await;
    for fs in closed {
        fs.close().await;
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
    if let Some(t) = sftp.0.transfers.lock().unwrap().get(&transfer) {
        t.cancel.cancel();
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
    on_progress: Channel<Progress>,
) -> CmdResult<Option<String>> {
    let fs = sftp.get(id)?;
    let mut dialog = dialog(&app).set_file_name(local_file_name(&name));
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
    let local: PathBuf = local.into_path().map_err(|e| CmdError::msg(e.to_string()))?;

    let (transfer, cancel) = sftp.start_transfer(id, &on_progress);
    let result = fs
        .download(&dir, &name, &local, &cancel, throttled(transfer, on_progress))
        .await;
    sftp.end_transfer(transfer);
    result?;
    Ok(Some(local.display().to_string()))
}

/// Native multi-file picker for uploads. Each picked regular file is opened
/// now and gets a single-use token; nothing is read yet.
#[tauri::command]
pub async fn sftp_pick_upload(app: AppHandle, sftp: State<'_, SftpState>) -> CmdResult<Vec<PickedFile>> {
    let (tx, rx) = oneshot::channel();
    dialog(&app).pick_files(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(files) = rx.await.ok().flatten() else {
        return Ok(Vec::new());
    };
    let mut picked = sftp.0.picked.lock().unwrap();
    picked.retain(|_, p| p.at.elapsed() < PICK_TTL);
    let mut out = Vec::new();
    for file in files {
        if picked.len() >= MAX_PICKED {
            return Err(CmdError::msg("too many files are waiting to be uploaded"));
        }
        let Ok(path) = file.into_path() else { continue };
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some((handle, size, mode)) = open_regular(&path) else {
            continue;
        };
        let token = Uuid::new_v4();
        picked.insert(
            token,
            Picked {
                file: handle,
                mode,
                name: name.clone(),
                at: Instant::now(),
            },
        );
        out.push(PickedFile { token, name, size });
    }
    Ok(out)
}

/// Releases tokens the UI did not use (skipped or stopped uploads).
#[tauri::command]
pub fn sftp_forget_picks(sftp: State<'_, SftpState>, tokens: Vec<Uuid>) {
    let mut picked = sftp.0.picked.lock().unwrap();
    for token in tokens {
        picked.remove(&token);
    }
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
        .0
        .picked
        .lock()
        .unwrap()
        .remove(&token)
        .filter(|p| p.at.elapsed() < PICK_TTL)
        .ok_or_else(|| CmdError::msg("pick the files again"))?;

    let (transfer, cancel) = sftp.start_transfer(id, &on_progress);
    let result = fs
        .upload(
            picked.file,
            picked.mode,
            &dir,
            &picked.name,
            overwrite,
            &cancel,
            throttled(transfer, on_progress),
        )
        .await;
    sftp.end_transfer(transfer);
    Ok(result.map(|_| ())?)
}

/// Opens `path` if it is a regular file, without blocking on FIFOs or
/// devices. Returns the open file, its size and its permission bits.
fn open_regular(path: &Path) -> Option<(std::fs::File, u64, Option<u32>)> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let mode = None;
    Some((file, meta.len(), mode))
}

/// Opening a FIFO must not hang; the flag has no effect on regular files,
/// the only kind kept.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
const O_NONBLOCK: i32 = 0x0004;
#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))))]
const O_NONBLOCK: i32 = 0o4000;

/// Native dialog attached to the main window, with the system's own title.
fn dialog(app: &AppHandle) -> FileDialogBuilder<tauri::Wry> {
    let mut d = app.dialog().file();
    if let Some(window) = app.get_webview_window("main") {
        d = d.set_parent(&window);
    }
    d
}

/// A safe default name for the save dialog: the server's name without path
/// separators, characters Windows rejects, or invisible characters.
fn local_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !is_format_char(*c))
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).trim_start_matches('.');
    let stem = cleaned.split('.').next().unwrap_or_default().to_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.chars().count() == 4
            && stem
                .chars()
                .nth(3)
                .is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)));
    match cleaned {
        "" => "download".to_owned(),
        c if reserved => format!("_{c}"),
        c => c.chars().take(200).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::local_file_name;

    #[test]
    fn local_names() {
        assert_eq!(local_file_name("report.pdf"), "report.pdf");
        assert_eq!(local_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(local_file_name("a\\b:c*?.txt"), "a_b_c__.txt");
        assert_eq!(local_file_name("evil\u{202e}gpj.exe"), "evilgpj.exe");
        assert_eq!(local_file_name("zero\u{200b}width"), "zerowidth");
        assert_eq!(local_file_name("..."), "download");
        assert_eq!(local_file_name(".bashrc"), "bashrc");
        assert_eq!(local_file_name("con.txt"), "_con.txt");
        assert_eq!(local_file_name("COM¹.log"), "_COM¹.log");
        assert_eq!(local_file_name("conout$"), "_conout$");
        assert_eq!(local_file_name("trailing. "), "trailing");
    }
}
