//! "Edit" in the file browser: a remote text file is copied to a private
//! temporary folder, opened in the system's *text editor* (never run, never
//! opened with whatever program its extension names), and uploaded back each
//! time it is saved there. If the file changed on the server meanwhile, the
//! user is asked before it is overwritten.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use termdock_core::sftp::{CancellationToken, Sftp};

use crate::commands::{CmdError, CmdResult};
use crate::sftp::SftpState;

/// Larger files are better downloaded and handled by hand.
const MAX_EDIT_SIZE: u64 = 5 * 1024 * 1024;
const MAX_EDITS: usize = 20;
/// How often the local copy is checked for a save.
const POLL: Duration = Duration::from_millis(800);

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
    dir: String,
    name: String,
    path: PathBuf,
    /// Removed (with the copy) when the edit ends.
    _folder: tempfile::TempDir,
    /// The server's size and time when last read or written.
    remote: (u64, Option<u32>),
    /// The local copy as last uploaded (or downloaded).
    synced: Vec<u8>,
    /// Local copy's (time, length) at the last check.
    seen: Option<(SystemTime, u64)>,
    /// Waiting for the user after a conflict.
    paused: bool,
    events: Channel<EditEvent>,
}

#[derive(Default)]
pub struct EditState {
    next: AtomicU32,
    edits: Mutex<HashMap<u32, Edit>>,
}

impl EditState {
    /// Ends every edit (the app is closing): copies are deleted.
    pub fn clear(&self) {
        self.edits.lock().unwrap().clear();
    }

    /// Ends the edits of a closed browser (their copies are deleted).
    pub fn forget_browser(&self, browser: u32) {
        self.edits.lock().unwrap().retain(|_, e| e.browser != browser);
    }
}

/// Copies left by an earlier run that did not exit cleanly (crash, killed):
/// deleted when more than a day old, so a second running instance keeps its own.
pub fn remove_stale_copies() {
    let Ok(dir) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("termdock-edit-") {
            continue;
        }
        // DirEntry::metadata does not follow links.
        let old = entry
            .metadata()
            .ok()
            .filter(|m| m.is_dir() && !m.file_type().is_symlink() && owned_by_us(m))
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 3600));
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
    if edits.edits.lock().unwrap().len() >= MAX_EDITS {
        return Err(CmdError::msg(
            "too many files are open for editing: close a file browser first",
        ));
    }
    let (size, mtime, kind) = fs.file_info(&dir, &name).await?;
    if kind != termdock_core::sftp::FileType::File {
        return Err(CmdError::msg("only regular files can be edited"));
    }
    if size > MAX_EDIT_SIZE {
        return Err(CmdError::msg(
            "the file is too large to edit here (more than 5 MB): download it instead",
        ));
    }
    // Private folder (0700 on Unix, set below); the copy is written 0600.
    let folder = tempfile::Builder::new()
        .prefix("termdock-edit-")
        .tempdir()
        .map_err(termdock_core::Error::from)?;
    // Only this user may even list it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(folder.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(termdock_core::Error::from)?;
    }
    let path = folder.path().join(editor_file_name(&name));
    fs.download(&dir, &name, &path, &CancellationToken::new(), |_, _| {})
        .await?;
    let content = std::fs::read(&path).map_err(termdock_core::Error::from)?;
    if content.iter().take(64 * 1024).any(|&b| b == 0) {
        return Err(CmdError::msg("this is not a text file: download it instead"));
    }
    open_in_text_editor(&path).map_err(|e| CmdError::msg(format!("could not open a text editor: {e}")))?;

    let edit_id = edits.next.fetch_add(1, Ordering::Relaxed) + 1;
    let seen = stamp(&path);
    edits.edits.lock().unwrap().insert(
        edit_id,
        Edit {
            browser: id,
            sftp: fs,
            dir,
            name,
            path,
            _folder: folder,
            remote: (size, mtime),
            synced: content,
            seen,
            paused: false,
            events: on_event,
        },
    );
    tauri::async_runtime::spawn(watch(edits.inner().clone(), edit_id));
    Ok(edit_id)
}

/// After a conflict: `overwrite` uploads the local copy anyway; otherwise the
/// edit stops (the server's version stays).
#[tauri::command]
pub async fn sftp_edit_resolve(edits: State<'_, Arc<EditState>>, edit: u32, overwrite: bool) -> CmdResult<()> {
    if !overwrite {
        edits.edits.lock().unwrap().remove(&edit);
        return Ok(());
    }
    if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
        e.paused = false;
    }
    push(&edits, edit, true).await;
    Ok(())
}

/// Stops watching a file (its local copy is deleted).
#[tauri::command]
pub fn sftp_edit_stop(edits: State<'_, Arc<EditState>>, edit: u32) {
    edits.edits.lock().unwrap().remove(&edit);
}

async fn watch(edits: Arc<EditState>, edit: u32) {
    loop {
        tokio::time::sleep(POLL).await;
        let changed = {
            let mut map = edits.edits.lock().unwrap();
            let Some(e) = map.get_mut(&edit) else { return };
            let now = stamp(&e.path);
            let changed = now.is_some() && now != e.seen && !e.paused;
            if changed {
                e.seen = now;
            }
            changed
        };
        if changed {
            push(&edits, edit, false).await;
        }
    }
}

/// Uploads the local copy if its content changed. Unless `force`, first
/// checks that the server's copy is still the one that was opened.
async fn push(edits: &EditState, edit: u32, force: bool) {
    let Some((sftp, dir, name, path, remote, synced, events)) = edits.edits.lock().unwrap().get(&edit).map(|e| {
        (
            e.sftp.clone(),
            e.dir.clone(),
            e.name.clone(),
            e.path.clone(),
            e.remote,
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
    let result = async {
        if !force {
            let (size, mtime, _) = sftp.file_info(&dir, &name).await?;
            if (size, mtime) != remote {
                return Ok::<_, termdock_core::Error>(false);
            }
        }
        let file = std::fs::File::open(&path)?;
        sftp.upload(file, None, &dir, &name, true, &CancellationToken::new(), |_, _| {})
            .await?;
        Ok(true)
    }
    .await;
    match result {
        Ok(true) => {
            let after = sftp.file_info(&dir, &name).await.ok();
            if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
                e.synced = content;
                if let Some((size, mtime, _)) = after {
                    e.remote = (size, mtime);
                }
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

/// Opens `path` in the system's text editor: Notepad on Windows, the
/// default text editor on macOS (`open -t`), the text/plain handler on Linux.
fn open_in_text_editor(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut cmd = {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut c = std::process::Command::new(PathBuf::from(root).join("System32").join("notepad.exe"));
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
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    let mut child = cmd.spawn()?;
    // Reap it when it exits (xdg-open/open return at once; Notepad later).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
