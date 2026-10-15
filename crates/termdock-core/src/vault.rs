//! Secrets (passwords, key passphrases) are kept only in the operating
//! system credential vault: Windows Credential Manager, macOS Keychain or the
//! Secret Service (GNOME Keyring / KWallet) on Linux. Nothing secret is ever
//! written to Termdock's own files, logged, or sent back to the UI.

use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const SERVICE: &str = "dev.termdock.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    Password,
    KeyPassphrase,
}

impl SecretKind {
    fn tag(self) -> &'static str {
        match self {
            SecretKind::Password => "password",
            SecretKind::KeyPassphrase => "passphrase",
        }
    }
}

pub type Secret = Zeroizing<String>;

pub trait Vault: Send + Sync {
    fn get(&self, profile: Uuid, kind: SecretKind) -> Result<Option<Secret>>;
    fn set(&self, profile: Uuid, kind: SecretKind, secret: &str) -> Result<()>;
    fn delete(&self, profile: Uuid, kind: SecretKind) -> Result<()>;

    fn delete_all(&self, profile: Uuid) -> Result<()> {
        self.delete(profile, SecretKind::Password)?;
        self.delete(profile, SecretKind::KeyPassphrase)
    }
}

/// The platform credential vault.
#[derive(Debug, Default)]
pub struct OsVault;

impl OsVault {
    fn entry(profile: Uuid, kind: SecretKind) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, &format!("{profile}:{}", kind.tag())).map_err(|e| Error::Vault(e.to_string()))
    }

    /// Whether a credential vault is usable on this machine.
    pub fn available() -> bool {
        keyring::Entry::store_status().is_ok()
    }
}

impl Vault for OsVault {
    fn get(&self, profile: Uuid, kind: SecretKind) -> Result<Option<Secret>> {
        match Self::entry(profile, kind)?.get_password() {
            Ok(s) => Ok(Some(Zeroizing::new(s))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Vault(e.to_string())),
        }
    }

    fn set(&self, profile: Uuid, kind: SecretKind, secret: &str) -> Result<()> {
        Self::entry(profile, kind)?
            .set_password(secret)
            .map_err(|e| Error::Vault(e.to_string()))
    }

    fn delete(&self, profile: Uuid, kind: SecretKind) -> Result<()> {
        match Self::entry(profile, kind)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Vault(e.to_string())),
        }
    }
}

/// Volatile vault for tests and for machines without a credential store.
/// Secrets are zeroized when removed or dropped.
#[derive(Default)]
pub struct MemoryVault {
    items: Mutex<HashMap<(Uuid, SecretKind), Secret>>,
}

impl Vault for MemoryVault {
    fn get(&self, profile: Uuid, kind: SecretKind) -> Result<Option<Secret>> {
        Ok(self.items.lock().unwrap().get(&(profile, kind)).cloned())
    }

    fn set(&self, profile: Uuid, kind: SecretKind, secret: &str) -> Result<()> {
        self.items
            .lock()
            .unwrap()
            .insert((profile, kind), Zeroizing::new(secret.to_owned()));
        Ok(())
    }

    fn delete(&self, profile: Uuid, kind: SecretKind) -> Result<()> {
        self.items.lock().unwrap().remove(&(profile, kind));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_vault() {
        let v = MemoryVault::default();
        let id = Uuid::new_v4();
        assert!(v.get(id, SecretKind::Password).unwrap().is_none());
        v.set(id, SecretKind::Password, "hunter2").unwrap();
        assert_eq!(v.get(id, SecretKind::Password).unwrap().unwrap().as_str(), "hunter2");
        assert!(v.get(id, SecretKind::KeyPassphrase).unwrap().is_none());
        v.delete_all(id).unwrap();
        assert!(v.get(id, SecretKind::Password).unwrap().is_none());
    }
}
