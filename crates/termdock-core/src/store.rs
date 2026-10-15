//! Persistent storage for profiles and the favorites bar order.
//!
//! The file is plain JSON (it holds no secrets), written atomically
//! (temp file + rename) and, on Unix, readable by the owner only.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::profile::Profile;

const SCHEMA_VERSION: u32 = 1;
const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    version: u32,
    profiles: Vec<Profile>,
    /// Profile ids pinned to the favorites bar, in display order.
    favorites: Vec<Uuid>,
}

#[derive(Debug)]
pub struct ProfileStore {
    path: PathBuf,
    doc: Document,
}

impl ProfileStore {
    /// Opens (or initialises) the store at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let doc = match fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_FILE_SIZE => {
                return Err(Error::InvalidProfile("profile file is too large".into()));
            }
            Ok(_) => {
                let mut doc: Document = serde_json::from_slice(&fs::read(&path)?)?;
                if doc.version > SCHEMA_VERSION {
                    return Err(Error::InvalidProfile(format!(
                        "profile file was written by a newer version (schema {})",
                        doc.version
                    )));
                }
                doc.version = SCHEMA_VERSION;
                // Drop dangling favorites and duplicates left by manual edits.
                let mut seen = std::collections::HashSet::new();
                let ids: std::collections::HashSet<_> = doc.profiles.iter().map(|p| p.id).collect();
                doc.favorites.retain(|id| ids.contains(id) && seen.insert(*id));
                doc
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Document {
                version: SCHEMA_VERSION,
                ..Default::default()
            },
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, doc })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn profiles(&self) -> &[Profile] {
        &self.doc.profiles
    }

    pub fn favorites(&self) -> &[Uuid] {
        &self.doc.favorites
    }

    pub fn get(&self, id: Uuid) -> Result<&Profile> {
        self.doc
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or(Error::ProfileNotFound(id))
    }

    /// Inserts or replaces a profile (matched by id) and persists the store.
    pub fn upsert(&mut self, mut profile: Profile) -> Result<Profile> {
        profile.validate()?;
        match self.doc.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile.clone(),
            None => self.doc.profiles.push(profile.clone()),
        }
        self.save()?;
        Ok(profile)
    }

    pub fn remove(&mut self, id: Uuid) -> Result<Profile> {
        let idx = self
            .doc
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or(Error::ProfileNotFound(id))?;
        let removed = self.doc.profiles.remove(idx);
        self.doc.favorites.retain(|f| *f != id);
        self.save()?;
        Ok(removed)
    }

    pub fn set_favorite(&mut self, id: Uuid, favorite: bool) -> Result<()> {
        self.get(id)?;
        let present = self.doc.favorites.contains(&id);
        if favorite && !present {
            self.doc.favorites.push(id);
        } else if !favorite && present {
            self.doc.favorites.retain(|f| *f != id);
        } else {
            return Ok(());
        }
        self.save()
    }

    /// Replaces the favorites order. `order` must be a permutation of the
    /// current favorites, so a stale UI cannot silently drop or add pins.
    pub fn reorder_favorites(&mut self, order: Vec<Uuid>) -> Result<()> {
        let mut a = order.clone();
        let mut b = self.doc.favorites.clone();
        a.sort();
        b.sort();
        if a != b {
            return Err(Error::InvalidProfile("favorites order does not match".into()));
        }
        self.doc.favorites = order;
        self.save()
    }

    fn save(&self) -> Result<()> {
        let dir = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        create_private_dir(dir)?;

        let mut tmp = tempfile::Builder::new()
            .prefix(".profiles")
            .suffix(".tmp")
            .tempfile_in(dir)?;
        // tempfile creates the file with mode 0600 on Unix.
        serde_json::to_writer_pretty(tmp.as_file_mut(), &self.doc)?;
        tmp.as_file_mut().write_all(b"\n")?;
        tmp.as_file_mut().sync_all()?;
        tmp.persist(&self.path).map_err(|e| Error::Io(e.error))?;
        Ok(())
    }
}

/// Creates `dir` (and parents) so that, on Unix, only the owner can access it.
pub fn create_private_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        if !dir.exists() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{LocalShell, ProfileKind};

    fn local(name: &str) -> Profile {
        Profile::new(
            name,
            ProfileKind::Local(LocalShell {
                program: "/bin/sh".into(),
                args: vec![],
                cwd: None,
            }),
        )
    }

    #[test]
    fn roundtrip_and_favorites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg").join("profiles.json");

        let mut store = ProfileStore::open(&path).unwrap();
        let a = store.upsert(local("a")).unwrap();
        let b = store.upsert(local("b")).unwrap();
        store.set_favorite(a.id, true).unwrap();
        store.set_favorite(b.id, true).unwrap();
        store.reorder_favorites(vec![b.id, a.id]).unwrap();
        assert!(store.reorder_favorites(vec![b.id]).is_err());

        let mut store = ProfileStore::open(&path).unwrap();
        assert_eq!(store.profiles().len(), 2);
        assert_eq!(store.favorites(), &[b.id, a.id]);

        store.remove(b.id).unwrap();
        assert_eq!(store.favorites(), &[a.id]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let mode = fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    #[test]
    fn rejects_invalid_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProfileStore::open(dir.path().join("p.json")).unwrap();
        assert!(store.upsert(local("")).is_err());
        assert!(store.profiles().is_empty());
    }

    #[test]
    fn drops_dangling_favorites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        let ghost = Uuid::new_v4();
        fs::write(
            &path,
            format!(r#"{{"version":1,"profiles":[],"favorites":["{ghost}"]}}"#),
        )
        .unwrap();
        let store = ProfileStore::open(&path).unwrap();
        assert!(store.favorites().is_empty());
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        fs::write(&path, r#"{"version":99,"profiles":[],"favorites":[]}"#).unwrap();
        assert!(ProfileStore::open(&path).is_err());
    }
}
