//! IPC commands exposed to the UI. Every command is listed in `build.rs` and
//! explicitly allowed in `capabilities/default.json`.
//!
//! The UI never receives secrets: passwords/passphrases only travel UI ->
//! backend, and stored ones stay in the OS vault.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};
use termdock_core::profile::{self, Profile, ProfileKind};
use termdock_core::session::{EventSink, SessionEvent, TermSize};
use termdock_core::shells::{self, DetectedShell};
use termdock_core::ssh::{self, SshConnect};
use termdock_core::sshconfig;
use termdock_core::vault::OsVault;
use termdock_core::{Error as CoreError, local};
use uuid::Uuid;

use crate::prompts::{PromptAnswer, SessionNotice, UiInteraction};
use crate::sftp::SftpState;
use crate::state::AppState;

/// Errors are flattened to a message for the UI.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CmdError(String);

impl CmdError {
    pub fn msg(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<CoreError> for CmdError {
    fn from(e: CoreError) -> Self {
        Self(e.to_string())
    }
}

impl From<tauri_plugin_updater::Error> for CmdError {
    fn from(e: tauri_plugin_updater::Error) -> Self {
        Self(e.to_string())
    }
}

impl Serialize for CmdError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

/// Runs blocking work (files, the keychain, helper processes) on the
/// blocking pool, so an async command does not hold up an async worker.
pub(crate) async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| CmdError::msg(format!("background task failed: {e}")))
}

const MAX_INPUT_CHUNK: usize = 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    vault_available: bool,
    platform: &'static str,
}

#[tauri::command(async)]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        // The version the updater compares against (tauri.conf.json).
        version: app.package_info().version.to_string(),
        vault_available: OsVault::available(),
        platform: std::env::consts::OS,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileList {
    profiles: Vec<Profile>,
    favorites: Vec<Uuid>,
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> ProfileList {
    let store = state.store.lock().unwrap();
    ProfileList {
        profiles: store.profiles().to_vec(),
        favorites: store.favorites().to_vec(),
    }
}

// Commands that touch the keychain or files run off the main thread
// (`async`): a keychain unlock prompt must not freeze the window.

#[tauri::command(async)]
pub fn save_profile(state: State<'_, AppState>, profile: Profile, favorite: Option<bool>) -> CmdResult<Profile> {
    let mut store = state.store.lock().unwrap();
    // A saved password belongs to user@host:port; if those change, forget it
    // instead of sending it to a different server. First, so that a failure
    // to forget it leaves the profile unchanged.
    if let Some(ProfileKind::Ssh(old)) = store.get(profile.id).ok().map(|p| &p.kind) {
        let moved = match &profile.kind {
            ProfileKind::Ssh(new) => {
                old.host != new.host || old.port != new.port || old.username != new.username || old.auth != new.auth
            }
            _ => true,
        };
        if moved {
            state.vault.delete_all(profile.id)?;
        }
    }
    let saved = store.upsert(profile)?;
    if let Some(fav) = favorite {
        store.set_favorite(saved.id, fav)?;
    }
    Ok(saved)
}

#[tauri::command(async)]
pub fn delete_profile(state: State<'_, AppState>, id: Uuid) -> CmdResult<()> {
    let mut store = state.store.lock().unwrap();
    if store.is_jump_host(id) {
        let users: Vec<_> = store
            .profiles()
            .iter()
            .filter(|p| matches!(&p.kind, ProfileKind::Ssh(t) if t.jump == Some(id)))
            .map(|p| p.name.as_str())
            .collect();
        return Err(CmdError(format!("used as jump host by: {}", users.join(", "))));
    }
    // Secrets first: if they cannot be removed, the profile stays (no
    // orphaned password left in the keychain).
    state.vault.delete_all(id)?;
    store.remove(id)?;
    Ok(())
}

#[tauri::command(async)]
pub fn set_favorite(state: State<'_, AppState>, id: Uuid, favorite: bool) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().set_favorite(id, favorite)?)
}

#[tauri::command(async)]
pub fn reorder_favorites(state: State<'_, AppState>, order: Vec<Uuid>) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().reorder_favorites(order)?)
}

/// Removes passwords/passphrases remembered for a profile.
#[tauri::command(async)]
pub fn forget_secrets(state: State<'_, AppState>, id: Uuid) -> CmdResult<()> {
    Ok(state.vault.delete_all(id)?)
}

#[tauri::command(async)]
pub fn forget_host_key(state: State<'_, AppState>, host: String, port: u16) -> CmdResult<usize> {
    Ok(state.known_hosts.forget(&host, port)?)
}

#[tauri::command(async)]
pub fn list_shells() -> Vec<DetectedShell> {
    shells::detect()
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenTarget {
    Profile {
        id: Uuid,
    },
    /// `user@host:port` typed in the quick-connect box.
    Quick {
        address: String,
    },
    /// One of the shells returned by `list_shells`, by id.
    Shell {
        id: String,
    },
}

#[tauri::command]
pub async fn open_session(
    state: State<'_, AppState>,
    sftp: State<'_, SftpState>,
    logs: State<'_, crate::sessionlog::Logs>,
    target: OpenTarget,
    size: TermSize,
    on_data: Channel<InvokeResponseBody>,
    on_notice: Channel<SessionNotice>,
) -> CmdResult<u32> {
    let id = state.next_session.fetch_add(1, Ordering::Relaxed);

    let sessions = state.sessions.clone();
    let browsers = sftp.inner().clone();
    let logs = logs.inner().clone();
    let exit_notices = on_notice.clone();
    // A shell can exit before connect/spawn returns: then the handle must
    // not be registered afterwards as if it were alive.
    let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let exited_flag = exited.clone();
    let sink: EventSink = Arc::new(move |event| match event {
        SessionEvent::Data(bytes) => {
            logs.write(id, &bytes);
            let _ = on_data.send(InvokeResponseBody::Raw(bytes));
        }
        SessionEvent::Exit { code, message } => {
            let mut sessions = sessions.lock().unwrap();
            exited_flag.store(true, Ordering::SeqCst);
            sessions.remove(&id);
            drop(sessions);
            browsers.session_ended(id);
            logs.stop(id);
            let _ = exit_notices.send(SessionNotice::Exit { code, message });
        }
    });

    let mut jumps = Vec::new();
    let (profile_id, kind) = match target {
        OpenTarget::Profile { id } => {
            let store = state.store.lock().unwrap();
            let p = store.get(id)?.clone();
            if let ProfileKind::Ssh(t) = &p.kind {
                jumps = profile::resolve_jumps(store.profiles(), t)?;
            }
            (Some(p.id), p.kind)
        }
        OpenTarget::Quick { address } => (
            None,
            ProfileKind::Ssh(profile::parse_quick_connect(&address, &default_user())?),
        ),
        OpenTarget::Shell { id } => {
            let shell = shells::detect()
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| CmdError(format!("unknown shell '{id}'")))?;
            (None, ProfileKind::Local(shell.shell))
        }
    };

    let handle = match kind {
        ProfileKind::Local(shell) => local::spawn(&shell, size, sink)?,
        ProfileKind::Ssh(target) => {
            let ui = Arc::new(UiInteraction {
                prompts: state.prompts.clone(),
                notices: on_notice,
            });
            ssh::connect(
                SshConnect {
                    profile_id,
                    target,
                    jumps,
                    size,
                    known_hosts: state.known_hosts.clone(),
                    vault: state.vault.clone(),
                    ui,
                },
                sink,
            )
            .await?
        }
    };
    {
        // Checked under the same lock the exit path takes.
        let mut sessions = state.sessions.lock().unwrap();
        if !exited.load(Ordering::SeqCst) {
            sessions.insert(id, handle);
        }
    }
    Ok(id)
}

/// `binary`: `data` carries raw bytes, one per char (xterm.js `onBinary`),
/// which must not be UTF-8 encoded.
#[tauri::command]
pub fn write_session(state: State<'_, AppState>, id: u32, data: String, binary: Option<bool>) -> CmdResult<()> {
    if data.len() > MAX_INPUT_CHUNK {
        return Err(CmdError("input too large".into()));
    }
    let bytes = if binary.unwrap_or(false) {
        data.chars()
            .map(|c| u8::try_from(u32::from(c)).map_err(|_| CmdError("invalid binary input".into())))
            .collect::<CmdResult<Vec<u8>>>()?
    } else {
        data.into_bytes()
    };
    let handle = state.sessions.lock().unwrap().get(&id).cloned();
    match handle {
        Some(h) => Ok(h.write(bytes)?),
        None => Err(CoreError::SessionClosed.into()),
    }
}

#[tauri::command]
pub fn resize_session(state: State<'_, AppState>, id: u32, size: TermSize) -> CmdResult<()> {
    if let Some(h) = state.sessions.lock().unwrap().get(&id) {
        h.resize(size)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn close_session(
    state: State<'_, AppState>,
    sftp: State<'_, SftpState>,
    logs: State<'_, crate::sessionlog::Logs>,
    id: u32,
) -> CmdResult<()> {
    // File transfers on this connection are cancelled and cleaned up first.
    sftp.close_session(id).await;
    logs.stop(id);
    if let Some(h) = state.sessions.lock().unwrap().remove(&id) {
        h.close();
    }
    Ok(())
}

#[tauri::command]
pub fn answer_prompt(state: State<'_, AppState>, prompt_id: Uuid, answer: PromptAnswer) -> bool {
    state.prompts.answer(prompt_id, answer)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConfigPreview {
    path: String,
    found: bool,
    items: Vec<sshconfig::PreviewItem>,
    warnings: Vec<String>,
}

fn read_ssh_config() -> CmdResult<(std::path::PathBuf, Option<sshconfig::Parsed>)> {
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_owned())
        .ok_or_else(|| CmdError("home directory not found".into()))?;
    let path = home.join(".ssh").join("config");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((path, Some(sshconfig::parse(&text, &home, &default_user())))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((path, None)),
        Err(e) => Err(CmdError(format!("{}: {e}", path.display()))),
    }
}

/// Reads `~/.ssh/config` (fixed path: the UI cannot choose which file the
/// backend reads) and lists what could be imported.
#[tauri::command(async)]
pub fn preview_ssh_config(state: State<'_, AppState>) -> CmdResult<SshConfigPreview> {
    let (path, parsed) = read_ssh_config()?;
    let store = state.store.lock().unwrap();
    Ok(match parsed {
        Some(parsed) => SshConfigPreview {
            path: path.display().to_string(),
            found: true,
            items: sshconfig::preview(&parsed, store.profiles()),
            warnings: parsed.warnings,
        },
        None => SshConfigPreview {
            path: path.display().to_string(),
            found: false,
            items: Vec::new(),
            warnings: Vec::new(),
        },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    imported: Vec<String>,
    warnings: Vec<String>,
}

#[tauri::command(async)]
pub fn import_ssh_config(state: State<'_, AppState>, aliases: Vec<String>) -> CmdResult<ImportResult> {
    let (_, parsed) = read_ssh_config()?;
    let parsed = parsed.ok_or_else(|| CmdError("~/.ssh/config not found".into()))?;
    let mut store = state.store.lock().unwrap();
    let (profiles, warnings) = sshconfig::plan_import(&parsed, store.profiles(), &aliases);
    let mut imported = Vec::new();
    for p in profiles {
        imported.push(store.upsert(p)?.name);
    }
    Ok(ImportResult { imported, warnings })
}

fn default_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".into())
}
