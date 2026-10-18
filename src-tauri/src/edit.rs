//! "Edit" in the file browser: a remote text file is copied to a private
//! temporary folder, opened in the system's *text editor* or the one chosen
//! in Settings (never run, never opened with whatever program its content or
//! extension suggests; see `editor`), and
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
use termdock_core::sftp::{CancellationToken, Sftp, validate_name};

use crate::commands::{CmdError, CmdResult};
use crate::editor::EditorChoice;
use crate::sftp::SftpState;

/// Larger files are better downloaded and handled by hand.
const MAX_EDIT_SIZE: u64 = 5 * 1024 * 1024;
const MAX_EDITS: usize = 20;
/// Copies of finished edits kept until exit.
const MAX_FINISHED: usize = 50;
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
    /// No longer watched (its file browser closed or switched user): saves
    /// are not uploaded any more.
    Stopped { edit: u32 },
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
    synced: Arc<Vec<u8>>,
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

    /// Stops watching the edits of a closed browser, telling the UI.
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
            if let Some(events) = self.stop(id) {
                let _ = events.send(EditEvent::Stopped { edit: id });
            }
        }
    }

    /// Ends an edit; its folder is kept until exit (the editor may still
    /// have the file open), at most [`MAX_FINISHED`] of them.
    fn stop(&self, edit: u32) -> Option<Channel<EditEvent>> {
        let removed = self.edits.lock().unwrap().remove(&edit)?;
        let mut finished = self.finished.lock().unwrap();
        finished.push(removed.folder);
        if finished.len() > MAX_FINISHED {
            // Deletes the oldest copy.
            finished.remove(0);
        }
        Some(removed.events)
    }

    /// Ends an edit whose editor never opened: its copy goes at once.
    fn discard(&self, edit: u32) {
        self.edits.lock().unwrap().remove(&edit);
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
    editor: State<'_, EditorChoice>,
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
    let folder = private_folder()?;
    let path = folder.path().join(editor_file_name(&name));
    let downloaded = fs
        .download_at_most(&dir, &name, &path, MAX_EDIT_SIZE, &CancellationToken::new(), |_, _| {})
        .await;
    // The type is checked on the opened file (no separate stat).
    if let Err(termdock_core::Error::NotRegular(_)) = downloaded {
        return Err(CmdError::msg("only regular files can be edited"));
    }
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
                synced: Arc::new(content),
                settling: None,
                paused: false,
                events: on_event,
            },
        );
    }
    if let Err(e) = editor.open(&path) {
        edits.discard(edit_id);
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
        let _ = edits.stop(edit);
        return Ok(());
    }
    push(&edits, &sftp, edit, true).await;
    if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
        e.paused = false;
    }
    Ok(())
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
    // The size first: a huge file is not read at all.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_EDIT_SIZE) {
        let _ = events.send(EditEvent::Failed {
            edit,
            message: "the file is larger than 5 MB".into(),
        });
        return;
    }
    let Ok(content) = std::fs::read(&path) else { return };
    if content == *synced && !force {
        return;
    }
    // A tracked transfer: closing the browser cancels it and waits for it.
    let Ok((transfer, cancel)) = sftp_state.track(browser) else {
        return;
    };
    let result = async {
        if !force {
            // Changed on the server (or grown past the limit): ask. Any other
            // error is a failed save, not a conflict.
            match sftp.read_at_most(&dir, &name, MAX_EDIT_SIZE).await {
                Ok(remote) if remote == *synced => {}
                Ok(_) | Err(termdock_core::Error::TooLarge) => return Ok::<_, termdock_core::Error>(false),
                Err(e) => return Err(e),
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
    sftp_state.end_transfer(transfer);
    match result {
        Ok(true) => {
            if let Some(e) = edits.edits.lock().unwrap().get_mut(&edit) {
                e.synced = Arc::new(content);
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
