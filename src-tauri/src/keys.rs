//! SSH keys for the user: a new key pair saved where the user picks it in a
//! save dialog, a public key picked in an open dialog, and installing a
//! public key on the server of an open session (like `ssh-copy-id`). Paths
//! come only from native dialogs; the passphrase only travels UI -> backend
//! and is wiped after use.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};
use termdock_core::keygen::{self, KeyPair, PublicKeyLine};
use termdock_core::sftp::Sftp;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::commands::{CmdError, CmdResult, blocking};
use crate::sftp::dialog;
use crate::state::AppState;

/// What `ssh-keygen` asks for at least, when there is a passphrase.
const MIN_PASSPHRASE: usize = 5;
/// A `.pub` file larger than this is not read.
const MAX_PUBLIC_FILE: u64 = 16 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    /// Where the private key was saved (generation only).
    path: Option<String>,
    /// The `authorized_keys` line.
    public_key: String,
    fingerprint: String,
    comment: String,
}

impl KeyInfo {
    fn new(public: PublicKeyLine, path: Option<&Path>) -> Self {
        Self {
            path: path.map(|p| p.display().to_string()),
            public_key: public.line,
            fingerprint: public.fingerprint,
            comment: public.comment,
        }
    }
}

/// `~/.ssh`; with `create`, made (`0700`) if missing, as `ssh-keygen` does.
fn ssh_dir(create: bool) -> Option<PathBuf> {
    let dir = directories::BaseDirs::new()?.home_dir().join(".ssh");
    if !dir.exists() {
        if !create {
            return None;
        }
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir).ok()?;
    }
    Some(dir)
}

/// `id_ed25519`, or the first `id_ed25519_N` with neither file taken.
fn free_name(dir: &Path) -> String {
    (1..100)
        .map(|n| {
            if n == 1 {
                "id_ed25519".to_owned()
            } else {
                format!("id_ed25519_{n}")
            }
        })
        .find(|name| {
            let path = dir.join(name);
            !path.exists() && !keygen::public_path(&path).exists()
        })
        .unwrap_or_else(|| "id_ed25519_termdock".into())
}

/// "user@computer", like `ssh-keygen`'s default comment.
fn default_comment() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "termdock".into());
    let comment: String = format!("{user}@{host}")
        .chars()
        .filter(|c| !c.is_control())
        .take(keygen::MAX_COMMENT)
        .collect();
    comment.trim_start_matches('@').to_owned()
}

/// Generates an Ed25519 key pair and saves it where the user picks (by
/// default a free `~/.ssh/id_ed25519…`). An existing file is never
/// replaced. `passphrase`: empty for none. `None` when the dialog was
/// cancelled.
#[tauri::command]
pub async fn ssh_key_generate(app: AppHandle, comment: String, passphrase: String) -> CmdResult<Option<KeyInfo>> {
    let passphrase = Zeroizing::new(passphrase);
    let comment = match comment.trim() {
        "" => default_comment(),
        c => c.to_owned(),
    };
    keygen::validate_comment(&comment)?;
    if !passphrase.is_empty() && passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(CmdError::msg(format!(
            "the passphrase must have at least {MIN_PASSPHRASE} characters"
        )));
    }

    let (tx, rx) = oneshot::channel();
    let mut picker = dialog(&app);
    if let Some(dir) = blocking(|| ssh_dir(true)).await? {
        picker = picker.set_directory(&dir).set_file_name(free_name(&dir));
    }
    picker.save_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| CmdError::msg(e.to_string()))?;
    if path.exists() || keygen::public_path(&path).exists() {
        return Err(CmdError::msg(
            "a file with this name already exists: choose another name (an existing key is never replaced)",
        ));
    }

    let target = path.clone();
    let pair: KeyPair = blocking(move || -> termdock_core::Result<KeyPair> {
        let pair = keygen::generate(&comment, Some(&passphrase))?;
        keygen::save(&pair, &target)?;
        Ok(pair)
    })
    .await??;
    Ok(Some(KeyInfo::new(pair.public, Some(&path))))
}

/// Asks for a public key file (`.pub`, in `~/.ssh` by default) and returns
/// the key it holds. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn ssh_key_pick_public(app: AppHandle) -> CmdResult<Option<KeyInfo>> {
    let (tx, rx) = oneshot::channel();
    let mut picker = dialog(&app).add_filter("Public key", &["pub"]);
    if let Some(dir) = blocking(|| ssh_dir(false)).await? {
        picker = picker.set_directory(dir);
    }
    picker.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| CmdError::msg(e.to_string()))?;
    let public = blocking(move || -> termdock_core::Result<PublicKeyLine> {
        if std::fs::metadata(&path)?.len() > MAX_PUBLIC_FILE {
            return Err(termdock_core::Error::TooLarge);
        }
        keygen::parse_public(&std::fs::read_to_string(&path)?)
    })
    .await??;
    Ok(Some(KeyInfo::new(public, None)))
}

/// Adds `public_key` (checked again here) to `~/.ssh/authorized_keys` of
/// the user session `id` is logged in as, over its own connection. Returns
/// false when the key was already there.
#[tauri::command]
pub async fn ssh_key_install(state: State<'_, AppState>, id: u32, public_key: String) -> CmdResult<bool> {
    let key = keygen::parse_public(&public_key)?;
    let handle = state
        .sessions
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or(termdock_core::Error::SessionClosed)?;
    let sftp = Sftp::open(&handle).await?;
    let added = sftp.authorize_key(&key).await;
    sftp.close().await;
    Ok(added?)
}
