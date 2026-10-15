//! IPC commands exposed to the UI. Every command is listed in `build.rs` and
//! explicitly allowed in `capabilities/default.json`.
//!
//! The UI never receives secrets: passwords/passphrases only travel UI ->
//! backend, and stored ones stay in the OS vault.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use termdock_core::profile::{self, Profile, ProfileKind};
use termdock_core::session::{EventSink, SessionEvent, TermSize};
use termdock_core::shells::{self, DetectedShell};
use termdock_core::ssh::{self, SshConnect};
use termdock_core::vault::OsVault;
use termdock_core::{Error as CoreError, local};
use uuid::Uuid;

use crate::prompts::{PromptAnswer, SessionNotice, UiInteraction};
use crate::state::AppState;

/// Errors are flattened to a message for the UI.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CmdError(String);

impl From<CoreError> for CmdError {
    fn from(e: CoreError) -> Self {
        Self(e.to_string())
    }
}

impl Serialize for CmdError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

type CmdResult<T> = Result<T, CmdError>;

const MAX_INPUT_CHUNK: usize = 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    vault_available: bool,
    platform: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
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

#[tauri::command]
pub fn save_profile(state: State<'_, AppState>, profile: Profile, favorite: Option<bool>) -> CmdResult<Profile> {
    let mut store = state.store.lock().unwrap();
    let previous = store.get(profile.id).ok().cloned();
    let saved = store.upsert(profile)?;
    if let Some(fav) = favorite {
        store.set_favorite(saved.id, fav)?;
    }
    drop(store);

    // A saved password belongs to user@host:port; if those change, forget it
    // instead of sending it to a different server.
    if let (Some(ProfileKind::Ssh(old)), ProfileKind::Ssh(new)) = (previous.map(|p| p.kind), &saved.kind)
        && (old.host != new.host || old.port != new.port || old.username != new.username || old.auth != new.auth)
    {
        state.vault.delete_all(saved.id)?;
    }
    Ok(saved)
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, id: Uuid) -> CmdResult<()> {
    state.store.lock().unwrap().remove(id)?;
    state.vault.delete_all(id)?;
    Ok(())
}

#[tauri::command]
pub fn set_favorite(state: State<'_, AppState>, id: Uuid, favorite: bool) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().set_favorite(id, favorite)?)
}

#[tauri::command]
pub fn reorder_favorites(state: State<'_, AppState>, order: Vec<Uuid>) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().reorder_favorites(order)?)
}

/// Removes passwords/passphrases remembered for a profile.
#[tauri::command]
pub fn forget_secrets(state: State<'_, AppState>, id: Uuid) -> CmdResult<()> {
    Ok(state.vault.delete_all(id)?)
}

#[tauri::command]
pub fn forget_host_key(state: State<'_, AppState>, host: String, port: u16) -> CmdResult<usize> {
    Ok(state.known_hosts.forget(&host, port)?)
}

#[tauri::command]
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
    target: OpenTarget,
    size: TermSize,
    on_data: Channel<InvokeResponseBody>,
    on_notice: Channel<SessionNotice>,
) -> CmdResult<u32> {
    let id = state.next_session.fetch_add(1, Ordering::Relaxed);

    let sessions = state.sessions.clone();
    let exit_notices = on_notice.clone();
    let sink: EventSink = Arc::new(move |event| match event {
        SessionEvent::Data(bytes) => {
            let _ = on_data.send(InvokeResponseBody::Raw(bytes));
        }
        SessionEvent::Exit { code, message } => {
            sessions.lock().unwrap().remove(&id);
            let _ = exit_notices.send(SessionNotice::Exit { code, message });
        }
    });

    let (profile_id, kind) = match target {
        OpenTarget::Profile { id } => {
            let p = state.store.lock().unwrap().get(id)?.clone();
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
    state.sessions.lock().unwrap().insert(id, handle);
    Ok(id)
}

#[tauri::command]
pub fn write_session(state: State<'_, AppState>, id: u32, data: String) -> CmdResult<()> {
    if data.len() > MAX_INPUT_CHUNK {
        return Err(CmdError("input too large".into()));
    }
    let handle = state.sessions.lock().unwrap().get(&id).cloned();
    match handle {
        Some(h) => Ok(h.write(data.into_bytes())?),
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
pub fn close_session(state: State<'_, AppState>, id: u32) {
    if let Some(h) = state.sessions.lock().unwrap().remove(&id) {
        h.close();
    }
}

#[tauri::command]
pub fn answer_prompt(state: State<'_, AppState>, prompt_id: Uuid, answer: PromptAnswer) -> bool {
    state.prompts.answer(prompt_id, answer)
}

fn default_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".into())
}
