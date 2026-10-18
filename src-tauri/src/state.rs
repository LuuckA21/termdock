use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use termdock_core::hostkeys::KnownHosts;
use termdock_core::session::SessionHandle;
use termdock_core::store::ProfileStore;
use termdock_core::vault::{MemoryVault, OsVault, Vault};
use tracing::warn;

use crate::commands::Lost;
use crate::prompts::PendingPrompts;

pub struct AppState {
    pub store: Mutex<ProfileStore>,
    pub vault: Arc<dyn Vault>,
    pub known_hosts: Arc<KnownHosts>,
    pub sessions: Arc<Mutex<HashMap<u32, SessionHandle>>>,
    /// SSH sessions whose connection dropped, by id, until they reconnect
    /// or their terminal is closed.
    pub lost: Arc<Mutex<HashMap<u32, Lost>>>,
    pub prompts: Arc<PendingPrompts>,
    pub next_session: AtomicU32,
}

impl AppState {
    pub fn new(config_dir: &Path) -> termdock_core::Result<Self> {
        let vault: Arc<dyn Vault> = if OsVault::available() {
            Arc::new(OsVault)
        } else {
            warn!("no OS credential vault available; secrets will not be remembered");
            Arc::new(MemoryVault::default())
        };
        Ok(Self {
            store: Mutex::new(ProfileStore::open(config_dir.join("profiles.json"))?),
            vault,
            known_hosts: Arc::new(KnownHosts::new(config_dir.join("known_hosts")).with_openssh_user_file()),
            sessions: Arc::default(),
            lost: Arc::default(),
            prompts: Arc::default(),
            next_session: AtomicU32::new(1),
        })
    }

    pub fn close_all(&self) {
        let mut sessions = self.sessions.lock().unwrap();
        self.lost.lock().unwrap().clear();
        for (_, h) in sessions.drain() {
            h.close();
        }
    }
}
