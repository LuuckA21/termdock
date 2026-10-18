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
use termdock_core::sftp::{CancellationToken, Listing, OpenAs, Sftp, SudoPending, is_format_char};
use tokio::sync::oneshot;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::commands::{CmdError, CmdResult};
use crate::edit::EditState;
use crate::state::AppState;

/// Upload tokens are single use and expire.
const PICK_TTL: Duration = Duration::from_secs(10 * 60);
/// Open picked files at most (each holds a file handle until uploaded).
const MAX_PICKED: usize = 200;
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
    /// sudo attempts waiting for a password, by id.
    sudo: Mutex<HashMap<u32, PendingSudo>>,
    /// Files open in a local editor.
    edits: Arc<EditState>,
}

struct PendingSudo {
    session: u32,
    user: String,
    login_user: String,
    pending: SudoPending,
}

fn closed() -> CmdError {
    CmdError::msg("the connection of this file browser is closed")
}

/// A sudo prompt left unanswered this long is closed.
const SUDO_TTL: Duration = Duration::from_secs(5 * 60);

/// Cheap to clone, so the session's exit handler can hold it.
#[derive(Clone, Default)]
pub struct SftpState(Arc<Inner>);

impl SftpState {
    /// The edit state, managed separately for its commands.
    pub fn edits(&self) -> Arc<EditState> {
        self.0.edits.clone()
    }

    pub(crate) fn browser(&self, id: u32) -> CmdResult<Arc<Sftp>> {
        self.0
            .browsers
            .lock()
            .unwrap()
            .get(&id)
            .map(|b| b.sftp.clone())
            .ok_or_else(closed)
    }

    fn next_id(&self) -> u32 {
        self.0.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Removes browsers (all of `session`'s, or just `browser`) and cancels
    /// their transfers. Returns what to close and the transfers to wait for.
    /// `None` for both: every browser.
    fn detach(&self, session: Option<u32>, browser: Option<u32>) -> (Vec<Arc<Sftp>>, Vec<u32>) {
        let all = session.is_none() && browser.is_none();
        let mut browsers = self.0.browsers.lock().unwrap();
        let ids: Vec<u32> = browsers
            .iter()
            .filter(|(id, b)| all || Some(b.session) == session || Some(**id) == browser)
            .map(|(id, _)| *id)
            .collect();
        let closed = ids
            .iter()
            .filter_map(|id| browsers.remove(id))
            .map(|b| b.sftp)
            .collect();
        drop(browsers);
        // Their edited files stop being watched.
        for id in &ids {
            self.0.edits.forget_browser(*id);
        }
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

    /// Termdock is quitting: every transfer is cancelled and given a few
    /// seconds to remove its temporary files (local `.part` files, scratch
    /// files on the servers), then the channels close.
    pub async fn close_all(&self) {
        let pending: Vec<PendingSudo> = self.0.sudo.lock().unwrap().drain().map(|(_, p)| p).collect();
        for p in pending {
            p.pending.cancel().await;
        }
        let (closed, running) = self.detach(None, None);
        self.drain(&running).await;
        for sftp in closed {
            sftp.close().await;
        }
    }

    /// The terminal session ended by itself: drop its browsers now.
    pub fn session_ended(&self, session: u32) {
        let (closed, _) = self.detach(Some(session), None);
        drop(closed);
        // Dropped attempts close their channels themselves.
        self.0.sudo.lock().unwrap().retain(|_, p| p.session != session);
    }

    /// The terminal session is being closed: cancel its transfers, let them
    /// clean up, then close the SFTP channels (before the SSH connection).
    pub async fn close_session(&self, session: u32) {
        let pending: Vec<PendingSudo> = {
            let mut sudo = self.0.sudo.lock().unwrap();
            let ids: Vec<u32> = sudo
                .iter()
                .filter(|(_, p)| p.session == session)
                .map(|(id, _)| *id)
                .collect();
            ids.iter().filter_map(|id| sudo.remove(id)).collect()
        };
        for p in pending {
            p.pending.cancel().await;
        }
        let (closed, running) = self.detach(Some(session), None);
        self.drain(&running).await;
        for sftp in closed {
            sftp.close().await;
        }
    }

    /// Registers a transfer of `browser`, so closing the browser cancels it
    /// and waits for its cleanup. Refused once the browser is closed: added
    /// while the browser list is locked, so a closing browser cannot miss it.
    pub(crate) fn track(&self, browser: u32) -> CmdResult<(u32, CancellationToken)> {
        let browsers = self.0.browsers.lock().unwrap();
        if !browsers.contains_key(&browser) {
            return Err(closed());
        }
        let id = self.next_id();
        let cancel = CancellationToken::new();
        self.0.transfers.lock().unwrap().insert(
            id,
            Transfer {
                browser,
                cancel: cancel.clone(),
            },
        );
        Ok((id, cancel))
    }

    fn start_transfer(&self, browser: u32, progress: &Channel<Progress>) -> CmdResult<(u32, CancellationToken)> {
        let (id, cancel) = self.track(browser)?;
        // The UI learns the id at once, so Cancel works from the start.
        let _ = progress.send(Progress {
            transfer: id,
            done: 0,
            total: None,
        });
        Ok((id, cancel))
    }

    pub(crate) fn end_transfer(&self, id: u32) {
        self.0.transfers.lock().unwrap().remove(&id);
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Opened {
    #[serde(rename_all = "camelCase")]
    Open {
        id: u32,
        home: String,
        /// The user the browser acts as.
        user: String,
        /// The user the terminal logged in as.
        login_user: String,
    },
    /// sudo waits for a password: answer with `sftp_sudo_password(pending, …)`.
    #[serde(rename_all = "camelCase")]
    PasswordRequired {
        pending: u32,
        /// The password given was not accepted.
        retry: bool,
        /// Whose password sudo asks for, when it says.
        asks_for: Option<String>,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    transfer: u32,
    done: u64,
    total: Option<u64>,
}

#[derive(Clone, Serialize)]
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

/// Opens a file browser on a terminal's SSH connection, as the login user or,
/// through sudo, as `user`.
#[tauri::command]
pub async fn sftp_open(
    state: State<'_, AppState>,
    sftp: State<'_, SftpState>,
    session: u32,
    user: Option<String>,
) -> CmdResult<Opened> {
    let handle = live_session(&state, session)?;
    let login_user = handle.ssh_user().unwrap_or_default().to_owned();
    match user {
        None => {
            let fs = Sftp::open(&handle).await?;
            register(&state, &sftp, session, fs, login_user.clone(), login_user).await
        }
        Some(user) => {
            let result = Sftp::open_as(&handle, &user).await?;
            settle(&state, &sftp, session, user, login_user, result).await
        }
    }
}

/// Gives the password to the sudo waiting in `pending`. It goes to that sudo
/// only (it printed its prompt), is not kept, and a wrong one asks again.
#[tauri::command]
pub async fn sftp_sudo_password(
    state: State<'_, AppState>,
    sftp: State<'_, SftpState>,
    pending: u32,
    password: String,
) -> CmdResult<Opened> {
    let password = Zeroizing::new(password);
    let p = sftp
        .0
        .sudo
        .lock()
        .unwrap()
        .remove(&pending)
        .ok_or_else(|| CmdError::msg("sudo is no longer waiting: try again"))?;
    if live_session(&state, p.session).is_err() {
        p.pending.cancel().await;
        return Err(CmdError::msg("the terminal session is closed"));
    }
    let result = Sftp::continue_as(p.pending, &password).await?;
    settle(&state, &sftp, p.session, p.user, p.login_user, result).await
}

/// Stops a sudo attempt waiting for a password.
#[tauri::command]
pub async fn sftp_sudo_cancel(sftp: State<'_, SftpState>, pending: u32) -> CmdResult<()> {
    let removed = sftp.0.sudo.lock().unwrap().remove(&pending);
    if let Some(p) = removed {
        p.pending.cancel().await;
    }
    Ok(())
}

fn live_session(state: &State<'_, AppState>, session: u32) -> CmdResult<termdock_core::session::SessionHandle> {
    state
        .sessions
        .lock()
        .unwrap()
        .get(&session)
        .cloned()
        .ok_or_else(|| CmdError::msg("the terminal session is closed"))
}

/// Registers an opened browser, or keeps a sudo attempt that needs a password.
async fn settle(
    state: &State<'_, AppState>,
    sftp: &State<'_, SftpState>,
    session: u32,
    user: String,
    login_user: String,
    result: OpenAs,
) -> CmdResult<Opened> {
    match result {
        OpenAs::Opened(fs) => register(state, sftp, session, *fs, user, login_user).await,
        OpenAs::PasswordRequired { pending, retry } => {
            let asks_for = pending.asks_for().map(str::to_owned);
            let id = sftp.next_id();
            sftp.0.sudo.lock().unwrap().insert(
                id,
                PendingSudo {
                    session,
                    user,
                    login_user,
                    pending,
                },
            );
            // Unanswered for too long: the waiting sudo is closed.
            let state = sftp.inner().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(SUDO_TTL).await;
                let expired = state.0.sudo.lock().unwrap().remove(&id);
                if let Some(p) = expired {
                    p.pending.cancel().await;
                }
            });
            Ok(Opened::PasswordRequired {
                pending: id,
                retry,
                asks_for,
            })
        }
    }
}

/// Keeps the browser if its terminal is still open, so a browser can never
/// outlive its session.
async fn register(
    state: &State<'_, AppState>,
    sftp: &State<'_, SftpState>,
    session: u32,
    fs: Sftp,
    user: String,
    login_user: String,
) -> CmdResult<Opened> {
    let home = fs.canonicalize(".").await?;
    let id = sftp.next_id();
    {
        let mut browsers = sftp.0.browsers.lock().unwrap();
        if state.sessions.lock().unwrap().contains_key(&session) {
            browsers.insert(
                id,
                Browser {
                    session,
                    sftp: Arc::new(fs),
                },
            );
            return Ok(Opened::Open {
                id,
                home,
                user,
                login_user,
            });
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
    Ok(sftp.browser(id)?.list(&path).await?)
}

#[tauri::command]
pub async fn sftp_mkdir(sftp: State<'_, SftpState>, id: u32, dir: String, name: String) -> CmdResult<()> {
    Ok(sftp.browser(id)?.mkdir(&dir, &name).await?)
}

#[tauri::command]
pub async fn sftp_rename(sftp: State<'_, SftpState>, id: u32, dir: String, from: String, to: String) -> CmdResult<()> {
    Ok(sftp.browser(id)?.rename(&dir, &from, &to).await?)
}

#[tauri::command]
pub async fn sftp_remove(sftp: State<'_, SftpState>, id: u32, dir: String, name: String) -> CmdResult<()> {
    Ok(sftp.browser(id)?.remove(&dir, &name).await?)
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
    // Fails fast, before the dialog, if the connection is already gone.
    sftp.browser(id)?;
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
    // The dialog may have stayed open while the connection closed.
    let fs = sftp.browser(id)?;
    let (transfer, cancel) = sftp.start_transfer(id, &on_progress)?;
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
    let paths: Vec<PathBuf> = files.into_iter().filter_map(|f| f.into_path().ok()).collect();
    Ok(sftp.take(&paths)?.0)
}

impl SftpState {
    /// Opens the regular files among `paths` (chosen by the user in a native
    /// dialog or dropped from the system) and hands out a single-use token
    /// for each. Also returns how many folders were left out.
    fn take(&self, paths: &[PathBuf]) -> CmdResult<(Vec<PickedFile>, usize)> {
        // Only absolute paths: anything else would be resolved against the
        // app's own working folder.
        let paths: Vec<&PathBuf> = paths.iter().filter(|p| p.is_absolute()).collect();
        let room = |picked: &mut HashMap<Uuid, Picked>, more: usize| {
            picked.retain(|_, p| p.at.elapsed() < PICK_TTL);
            picked.len() + more <= MAX_PICKED
        };
        // Checked before opening anything, so nothing is opened on refusal.
        if !room(&mut self.0.picked.lock().unwrap(), paths.len()) {
            return Err(CmdError::msg("too many files at once"));
        }
        // Opened without holding the lock (a slow network share must not
        // block other commands).
        let mut opened = Vec::new();
        let mut folders = 0;
        for path in paths {
            if path.is_dir() {
                folders += 1;
                continue;
            }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if let Some((handle, size, mode)) = open_regular(path) {
                opened.push((name, handle, size, mode));
            }
        }
        let mut picked = self.0.picked.lock().unwrap();
        // Checked again: other picks may have arrived meanwhile.
        if !room(&mut picked, opened.len()) {
            return Err(CmdError::msg("too many files at once"));
        }
        let mut out = Vec::new();
        for (name, handle, size, mode) in opened {
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
        Ok((out, folders))
    }
}

/// Where files dragged from the system are over the window (CSS pixels),
/// sent to the UI so it can highlight the file browser under them.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DragOver {
    x: f64,
    y: f64,
    active: bool,
}

/// Files dropped on the window: tokens (not paths) and where they landed.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Dropped {
    x: f64,
    y: f64,
    files: Vec<PickedFile>,
    folders: usize,
}

/// Files dragged from the system onto the window. The paths come from the
/// operating system, never from the UI: dropped regular files are opened at
/// once and the UI only receives tokens, as with the open dialog. The UI
/// releases the tokens of a drop that did not land on a file browser.
pub fn on_drag_drop(window: &tauri::Window, event: &tauri::DragDropEvent) {
    use tauri::Emitter;
    // Only Windows reports physical pixels; WebKitGTK and WKWebView already
    // give CSS (logical) pixels.
    let scale = if cfg!(windows) {
        window.scale_factor().unwrap_or(1.0)
    } else {
        1.0
    };
    let at = |p: &tauri::PhysicalPosition<f64>| {
        let l = p.to_logical::<f64>(scale);
        (l.x, l.y)
    };
    let label = window.label().to_owned();
    match event {
        tauri::DragDropEvent::Enter { position, .. } | tauri::DragDropEvent::Over { position } => {
            let (x, y) = at(position);
            let _ = window.emit_to(label.as_str(), "termdock://drag", DragOver { x, y, active: true });
        }
        tauri::DragDropEvent::Leave => {
            let _ = window.emit_to(
                label.as_str(),
                "termdock://drag",
                DragOver {
                    x: 0.0,
                    y: 0.0,
                    active: false,
                },
            );
        }
        tauri::DragDropEvent::Drop { paths, position } => {
            let _ = window.emit_to(
                label.as_str(),
                "termdock://drag",
                DragOver {
                    x: 0.0,
                    y: 0.0,
                    active: false,
                },
            );
            let (x, y) = at(position);
            let window = window.clone();
            let paths = paths.clone();
            // Opening files (maybe on a slow share) off the event loop.
            tauri::async_runtime::spawn_blocking(move || {
                let state = window.state::<SftpState>();
                let (files, folders) = state.take(&paths).unwrap_or_default();
                let _ = window.emit_to(label.as_str(), "termdock://drop", Dropped { x, y, files, folders });
            });
        }
        _ => {}
    }
}

/// Releases tokens the UI did not use (skipped or stopped uploads).
#[tauri::command]
pub fn sftp_forget_picks(sftp: State<'_, SftpState>, tokens: Vec<Uuid>) {
    let mut picked = sftp.0.picked.lock().unwrap();
    for token in tokens {
        picked.remove(&token);
    }
    // Expired picks close their files now, not at the next pick.
    picked.retain(|_, p| p.at.elapsed() < PICK_TTL);
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
    let fs = sftp.browser(id)?;
    let picked = sftp
        .0
        .picked
        .lock()
        .unwrap()
        .remove(&token)
        .filter(|p| p.at.elapsed() < PICK_TTL)
        .ok_or_else(|| CmdError::msg("pick the files again"))?;

    let (transfer, cancel) = sftp.start_transfer(id, &on_progress)?;
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
pub(crate) fn dialog(app: &AppHandle) -> FileDialogBuilder<tauri::Wry> {
    let mut d = app.dialog().file();
    if let Some(window) = app.get_webview_window("main") {
        d = d.set_parent(&window);
    }
    d
}

/// A safe default name for the save dialog: the server's name without path
/// separators, characters Windows rejects, or invisible characters.
pub(crate) fn local_file_name(name: &str) -> String {
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
        c => {
            // At most 200 bytes, on a character boundary (file systems count bytes).
            let mut end = c.len().min(200);
            while !c.is_char_boundary(end) {
                end -= 1;
            }
            c[..end].to_owned()
        }
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
