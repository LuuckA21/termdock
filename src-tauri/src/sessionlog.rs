//! Session logs: what a terminal shows, written as plain text to a file the
//! user picks in a native save dialog (the UI never names the path). Off
//! unless started for a terminal; it ends with the session or on request.
//!
//! A log holds whatever the terminal showed, secrets printed on screen
//! included: it is created readable by the owner only (Unix), and the UI says
//! so before asking where to save it. Typed input is not logged as such (a
//! password typed at a prompt is not echoed, so it is not in the log either).

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, State};
use termdock_core::textlog::{Stripper, utc_stamp};
use tokio::sync::oneshot;

use crate::commands::{CmdError, CmdResult, blocking};
use crate::sftp::{dialog, local_file_name};
use crate::state::AppState;

/// Output chunks waiting to be written; past this the disk is too slow and
/// the excess is dropped (and the log says so).
const QUEUE: usize = 4096;

enum Entry {
    /// Terminal output, written as plain text.
    Output(Vec<u8>),
    /// A line of the log's own, with the time.
    Note(&'static str),
}

struct Log {
    tx: SyncSender<Entry>,
    lost: Arc<AtomicBool>,
}

/// Running logs, by session id.
#[derive(Default, Clone)]
pub struct Logs(Arc<Mutex<HashMap<u32, Log>>>);

impl Logs {
    /// Called with every chunk a session prints.
    pub fn write(&self, session: u32, bytes: &[u8]) {
        if let Some(log) = self.0.lock().unwrap().get(&session) {
            match log.tx.try_send(Entry::Output(bytes.to_vec())) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => log.lost.store(true, Ordering::Relaxed),
                Err(TrySendError::Disconnected(_)) => {}
            }
        }
    }

    /// Adds a line such as "connection lost" (with the time) to the log of
    /// `session`, if it has one.
    pub fn note(&self, session: u32, text: &'static str) {
        if let Some(log) = self.0.lock().unwrap().get(&session) {
            // Not worth blocking for; a full queue already says output is missing.
            let _ = log.tx.try_send(Entry::Note(text));
        }
    }

    /// Stops the log of `session` (the file is closed once the queue is written).
    pub fn stop(&self, session: u32) -> bool {
        self.0.lock().unwrap().remove(&session).is_some()
    }

    pub fn stop_all(&self) {
        self.0.lock().unwrap().clear();
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A file only its owner can read (Unix), replacing what the save dialog
/// already agreed to replace.
fn create_private(path: &PathBuf) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    // An existing file keeps its mode with open(): tighten it too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn writer(mut file: File, rx: std::sync::mpsc::Receiver<Entry>, lost: Arc<AtomicBool>) {
    let mut stripper = Stripper::new();
    let mut text = Vec::new();
    let mut warned = false;
    while let Ok(entry) = rx.recv() {
        text.clear();
        if !warned && lost.load(Ordering::Relaxed) {
            warned = true;
            text.extend_from_slice(b"\n# [some output is missing here: the disk was too slow]\n");
        }
        match entry {
            Entry::Output(chunk) => stripper.feed(&chunk, &mut text),
            Entry::Note(note) => {
                stripper.break_off(&mut text);
                text.extend_from_slice(format!("\n# {note} {}\n", utc_stamp(now())).as_bytes());
            }
        }
        if file.write_all(&text).is_err() {
            return;
        }
    }
    text.clear();
    stripper.break_off(&mut text);
    let _ = file.write_all(&text);
    let _ = writeln!(file, "\n# log ended {}", utc_stamp(now()));
}

/// Asks where to save, then logs session `id` there from now on. `title` is
/// the tab's title, for the file name and the first line. Returns the path,
/// or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn session_log_start(
    app: AppHandle,
    state: State<'_, AppState>,
    logs: State<'_, Logs>,
    id: u32,
    title: String,
) -> CmdResult<Option<String>> {
    if !state.sessions.lock().unwrap().contains_key(&id) {
        return Err(termdock_core::Error::SessionClosed.into());
    }
    let title: String = title.chars().filter(|c| !c.is_control()).take(100).collect();
    let stamp = utc_stamp(now());
    let name = local_file_name(&format!("{title} {}.log", &stamp[..10]));
    let (tx, rx) = oneshot::channel();
    dialog(&app)
        .set_file_name(name)
        .add_filter("Log", &["log", "txt"])
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| CmdError::msg(e.to_string()))?;
    let opened = {
        let path = path.clone();
        blocking(move || -> std::io::Result<File> {
            let mut file = create_private(&path)?;
            writeln!(file, "# Termdock session log: {title}\n# started {stamp}\n")?;
            Ok(file)
        })
        .await?
    };
    let file = opened.map_err(termdock_core::Error::from)?;
    let (chunks, queue) = sync_channel(QUEUE);
    let lost = Arc::new(AtomicBool::new(false));
    {
        // Registered only while the session still runs (checked under the
        // sessions lock, which its exit also takes).
        let sessions = state.sessions.lock().unwrap();
        if !sessions.contains_key(&id) {
            return Err(termdock_core::Error::SessionClosed.into());
        }
        logs.0.lock().unwrap().insert(
            id,
            Log {
                tx: chunks,
                lost: lost.clone(),
            },
        );
    }
    std::thread::spawn(move || writer(file, queue, lost));
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub fn session_log_stop(logs: State<'_, Logs>, id: u32) -> bool {
    logs.stop(id)
}
