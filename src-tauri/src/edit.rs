//! "Edit" in the file browser: a remote text file is copied to a private
//! temporary folder, opened in the system's *text editor* (never run, never
//! opened with whatever program its content or extension suggests), and
//! uploaded back each time it is saved there. If the file changed on the
//! server meanwhile, the user is asked before it is overwritten.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use termdock_core::sftp::{CancellationToken, FileType, Sftp, validate_name};

use crate::commands::{CmdError, CmdResult};
use crate::sftp::SftpState;

/// Larger files are better downloaded and handled by hand.
const MAX_EDIT_SIZE: u64 = 5 * 1024 * 1024;
const MAX_EDITS: usize = 20;
/// How often the local copy is checked for a save.
const POLL: Duration = Duration::from_millis(700);
/// Copies left by a run that did not exit cleanly are removed after this.
const STALE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EditEvent {
    /// Uploaded after a save in the editor.
    Saved { edit: u32 },
    /// The server's copy changed since it was opened: answer with `sftp_edit_resolve`.
    Conflict { edit: u32 },
    /// Saving failed; the local copy is kept, the next save tries again.
    Failed { edit: u32, message: String },
}

struct Edit {
    browser: u32,
    sftp: Arc<Sftp>,
    /// The file edited (a link is resolved to its target when opened).
    dir: String,
    name: String,
    path: PathBuf,
    folder: tempfile::TempDir,
    /// The content last read from or written to the server.
    synced: Vec<u8>,
    /// Local copy's (time, length) at the last upload or download.
    seen: Option<(SystemTime, u64)>,
    /// A change seen at the previous poll: uploaded once it stops changing.
    settling: Option<(SystemTime, u64)>,
    /// Waiting for the user after a conflict.
    paused: bool,
    events: Channel<EditEvent>,
}

#[derive(Default)]
pub struct EditState {
    next: AtomicU32,
    edits: Mutex<HashMap<u32, Edit>>,
    /// Folders of finished edits: the editor may still have the file open,
    /// so they are only deleted when Termdock exits.
    finished: Mutex<Vec<tempfile::TempDir>>,
}

impl EditState {
    /// Ends every edit (the app is closing): all copies are deleted.
    pub fn clear(&self) {
        self.edits.lock().unwrap().clear();
        self.finished.lock().unwrap().clear();
    }

    /// Stops watching the edits of a closed browser.
    pub fn forget_browser(&self, browser: u32) {
        let ended: Vec<u32> = {
            let edits = self.edits.lock().unwrap();
            edits
                .iter()
                .filter(|(_, e)| e.browser == browser)
                .map(|(id, _)| *id)
                .collect()
        };
        for id in ended {
            self.stop(id);
        }
    }

    fn stop(&self, edit: u32) {
        let removed = self.edits.lock().unwrap().remove(&edit);
        if let Some(e) = removed {
            self.finished.lock().unwrap().push(e.folder);
        }
    }
}

/// Copies left by an earlier run that did not exit cleanly (crash, killed):
/// deleted after a week, only this user's, so another running Termdock keeps
/// its own.
pub fn remove_stale_copies() {
    let Ok(dir) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in dir.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("termdock-edit-") {
            continue;
        }
        // DirEntry::metadata does not follow links.
        let old = entry
            .metadata()
            .ok()
            .filter(|m| m.is_dir() && !m.file_type().is_symlink() && owned_by_us(m))
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// On Linux /tmp is shared: only this user's folders are cleaned up.
#[cfg(unix)]
fn owned_by_us(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // /proc/self belongs to this process's user (Linux); elsewhere (macOS)
    // the temporary folder is per user anyway.
    match std::fs::metadata("/proc/self") {
        Ok(me) => meta.uid() == me.uid(),
        Err(_) => true,
    }
}

#[cfg(not(unix))]
fn owned_by_us(_meta: &std::fs::Metadata) -> bool {
    // %TEMP% is per user.
    true
}

/// Opens `dir/name` in the system text editor and uploads it back on save.
#[tauri::command]
pub async fn sftp_edit(
    sftp: State<'_, SftpState>,
    edits: State<'_, Arc<EditState>>,
    id: u32,
    dir: String,
    name: String,
    on_event: Channel<EditEvent>,
) -> CmdResult<u32> {
    let fs = sftp.browser(id)?;
    // A name that could not be saved back is refused now, not at the first save.
    validate_name(&name)?;
    if edits.edits.lock().unwrap().len() >= MAX_EDITS {
        return Err(too_many());
    }
    // A link is edited at its target (saving would otherwise replace the link).
    let (dir, name) = fs.resolve(&dir, &name).await?;
    validate_name(&name)?;
    let (_, _, kind) = fs.file_info(&dir, &name).await?;
    if kind != FileType::File {
        return Err(CmdError::msg("only regular files can be edited"));
    }
    let folder = private_folder()?;
    let path = folder.path().join(editor_file_name(&name));
    let downloaded = fs
        .download_at_most(&dir, &name, &path, MAX_EDIT_SIZE, &CancellationToken::new(), |_, _| {})
        .await;
    if let Err(termdock_core::Error::TooLarge) = downloaded {
        return Err(CmdError::msg(
            "the file is too large to edit here (more than 5 MB): download it instead",
        ));
    }
    downloaded?;
    let content = std::fs::read(&path).map_err(termdock_core::Error::from)?;
    if content.iter().take(64 * 1024).any(|&b| b == 0) {
        return Err(CmdError::msg("this is not a text file: download it instead"));
    }

    let edit_id = edits.next.fetch_add(1, Ordering::Relaxed) + 1;
    {
        // Checked again now: other edits may have started, or the browser
        // closed, while this one was downloading.
        let mut map = edits.edits.lock().unwrap();
        if map.len() >= MAX_EDITS {
            return Err(too_many());
        }
        if sftp.browser(id).is_err() {
            return Err(CmdError::msg("the file browser was closed"));
        }
        map.insert(
            edit_id,
            Edit {
                browser: id,
                sftp: fs,
                dir,
                name,
                seen: stamp(&path),
                path: path.clone(),
                folder,
                synced: content,
                settling: None,
                paused: false,
                events: on_event,
            },
        );
    }
    if let Err(e) = open_in_text_editor(&path) {
        edits.stop(edit_id);
        return Err(CmdError::msg(e));
    }
    tauri::async_runtime::spawn(watch(edits.inner().clone(), sftp.inner().clone(), edit_id));
    Ok(edit_id)
}

/// After a conflict: `overwrite` uploads the local copy anyway; otherwise the
/// edit stops (the server's version stays).
#[tauri::command]
pub async fn sftp_edit_resolve(
    sftp: State<'_, SftpState>,
    edits: State<'_, Arc<EditState>>,
    edit: u32,
    overwrite: bool,
) -> CmdResult<()> {
    if !overwrite {
        edits.stop(edit);
        return Ok(());
    }
    push(&edits, &sftp, edit, true).await;
    if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
        e.paused = false;
    }
    Ok(())
}

/// Stops watching a file.
#[tauri::command]
pub fn sftp_edit_stop(edits: State<'_, Arc<EditState>>, edit: u32) {
    edits.stop(edit);
}

fn too_many() -> CmdError {
    CmdError::msg("too many files are open for editing: close a file browser first")
}

/// A new folder only this user can open (0700 on Unix from the start).
fn private_folder() -> CmdResult<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("termdock-edit-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir().map_err(termdock_core::Error::from)?)
}

/// Polls the local copy; a change is uploaded once it has stopped changing
/// (so a file the editor is still writing is not sent half done).
async fn watch(edits: Arc<EditState>, sftp: SftpState, edit: u32) {
    loop {
        tokio::time::sleep(POLL).await;
        let ready = {
            let mut map = edits.edits.lock().unwrap();
            let Some(e) = map.get_mut(&edit) else { return };
            let now = stamp(&e.path);
            if e.paused || now.is_none() || now == e.seen {
                e.settling = None;
                false
            } else if now == e.settling {
                e.seen = now;
                e.settling = None;
                true
            } else {
                e.settling = now;
                false
            }
        };
        if ready {
            push(&edits, &sftp, edit, false).await;
        }
    }
}

/// Uploads the local copy if its content changed. Unless `force`, first
/// checks that the server still has the content that was opened (or last
/// saved); otherwise reports a conflict and waits for the user.
async fn push(edits: &EditState, sftp_state: &SftpState, edit: u32, force: bool) {
    let Some((browser, sftp, dir, name, path, synced, events)) = edits.edits.lock().unwrap().get(&edit).map(|e| {
        (
            e.browser,
            e.sftp.clone(),
            e.dir.clone(),
            e.name.clone(),
            e.path.clone(),
            e.synced.clone(),
            e.events.clone(),
        )
    }) else {
        return;
    };
    let Ok(content) = std::fs::read(&path) else { return };
    if content == synced && !force {
        return;
    }
    if content.len() as u64 > MAX_EDIT_SIZE {
        let _ = events.send(EditEvent::Failed {
            edit,
            message: "the file is larger than 5 MB".into(),
        });
        return;
    }
    // A tracked transfer: closing the browser cancels it and waits for it.
    let (transfer, cancel) = sftp_state.track(browser);
    let result = async {
        if !force {
            let remote = sftp.read_at_most(&dir, &name, MAX_EDIT_SIZE).await;
            if !matches!(remote, Ok(ref r) if *r == synced) {
                return Ok::<_, termdock_core::Error>(false);
            }
        }
        // Exactly the bytes read above (the editor may write again meanwhile).
        sftp.upload_from(
            &content[..],
            content.len() as u64,
            None,
            &dir,
            &name,
            true,
            &cancel,
            |_, _| {},
        )
        .await?;
        Ok(true)
    }
    .await;
    sftp_state.untrack(transfer);
    match result {
        Ok(true) => {
            if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
                e.synced = content;
            }
            let _ = events.send(EditEvent::Saved { edit });
        }
        Ok(false) => {
            if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
                e.paused = true;
            }
            let _ = events.send(EditEvent::Conflict { edit });
        }
        Err(err) => {
            let _ = events.send(EditEvent::Failed {
                edit,
                message: err.to_string(),
            });
        }
    }
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The copy's local name: safe for this OS. On Linux it ends in ".txt", so
/// the desktop opens it as text whatever the server called it.
fn editor_file_name(name: &str) -> String {
    let safe = crate::sftp::local_file_name(name);
    if cfg!(all(unix, not(target_os = "macos"))) && !safe.to_ascii_lowercase().ends_with(".txt") {
        format!("{safe}.txt")
    } else {
        safe
    }
}

/// Opens `path` in the system's text editor: Notepad by its full path on
/// Windows (see `windows_notepad`), the default text editor on macOS
/// (`open -t`). On Linux, the application registered for text/plain is
/// started on it (`gtk-launch`), or GIO opens it by its `.txt` name; anything
/// that would pick the program from the file's content is not used.
fn open_in_text_editor(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = {
        let notepad = windows_notepad().ok_or_else(|| "Notepad was not found: download the file instead".to_owned())?;
        let mut c = std::process::Command::new(notepad);
        c.arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("/usr/bin/open");
        c.arg("-t").arg(path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = linux_text_editor(path)
        .ok_or_else(|| "no text editor found (gtk-launch or gio is needed): download the file instead".to_owned())?;
    let mut child = cmd.spawn().map_err(|e| format!("could not open a text editor: {e}"))?;
    // Reap it when it exits (open/gio return at once; editors later).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Notepad's full path: the classic one in the Windows folder, or, where only
/// the Store version is installed (recent Windows 11), its app alias.
#[cfg(windows)]
fn windows_notepad() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));
    let alias = std::env::var_os("LOCALAPPDATA").map(|d| {
        PathBuf::from(d)
            .join("Microsoft")
            .join("WindowsApps")
            .join("notepad.exe")
    });
    [
        Some(root.join("System32").join("notepad.exe")),
        Some(root.join("notepad.exe")),
        alias,
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_absolute())
    // An app alias is a reparse point that cannot be followed: check the
    // entry itself.
    .find(|p| std::fs::symlink_metadata(p).is_ok())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_text_editor(path: &Path) -> Option<std::process::Command> {
    // The desktop's text/plain application, started explicitly.
    let handler = std::process::Command::new("xdg-mime")
        .args(["query", "default", "text/plain"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|id| {
            id.ends_with(".desktop")
                && id.len() < 256
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        });
    if let Some(id) = handler
        && on_path("gtk-launch")
    {
        let mut c = std::process::Command::new("gtk-launch");
        c.arg(id.trim_end_matches(".desktop")).arg(path);
        return Some(c);
    }
    // GIO picks the application from the name first: a .txt name is text.
    if on_path("gio") {
        let mut c = std::process::Command::new("gio");
        c.arg("open").arg(path);
        return Some(c);
    }
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}
