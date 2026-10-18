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
use crate::profile::{Profile, ProfileKind, resolve_jumps};

const SCHEMA_VERSION: u32 = 1;
const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024;
/// Most profiles one store (or an imported backup) holds.
pub const MAX_PROFILES: usize = 5_000;

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
        // The jump chain must exist, be SSH only and loop free, with this
        // profile as it is about to be saved.
        if let ProfileKind::Ssh(target) = &profile.kind
            && target.jump.is_some()
        {
            let mut all: Vec<Profile> = self
                .doc
                .profiles
                .iter()
                .filter(|p| p.id != profile.id)
                .cloned()
                .collect();
            all.push(profile.clone());
            resolve_jumps(&all, target)?;
        }
        if !matches!(profile.kind, ProfileKind::Ssh(_)) && self.is_jump_host(profile.id) {
            return Err(Error::InvalidProfile(
                "this connection is used as jump host and must stay SSH".into(),
            ));
        }
        let mut next = self.doc.clone();
        match next.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile.clone(),
            None => next.profiles.push(profile.clone()),
        }
        self.commit(next)?;
        Ok(profile)
    }

    /// Whether another profile connects through `id`.
    pub fn is_jump_host(&self, id: Uuid) -> bool {
        self.doc
            .profiles
            .iter()
            .any(|p| matches!(&p.kind, ProfileKind::Ssh(t) if t.jump == Some(id)))
    }

    pub fn remove(&mut self, id: Uuid) -> Result<Profile> {
        let idx = self
            .doc
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or(Error::ProfileNotFound(id))?;
        let mut next = self.doc.clone();
        let removed = next.profiles.remove(idx);
        next.favorites.retain(|f| *f != id);
        self.commit(next)?;
        Ok(removed)
    }

    pub fn set_favorite(&mut self, id: Uuid, favorite: bool) -> Result<()> {
        self.get(id)?;
        let present = self.doc.favorites.contains(&id);
        let mut next = self.doc.clone();
        if favorite && !present {
            next.favorites.push(id);
        } else if !favorite && present {
            next.favorites.retain(|f| *f != id);
        } else {
            return Ok(());
        }
        self.commit(next)
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
        let mut next = self.doc.clone();
        next.favorites = order;
        self.commit(next)
    }

    /// Replaces every profile and the favorites at once (a backup imported
    /// with "replace"), checked like [`ProfileStore::upsert`]: all or nothing.
    pub fn replace_all(&mut self, profiles: Vec<Profile>, favorites: Vec<Uuid>) -> Result<()> {
        let next = checked(Document {
            version: SCHEMA_VERSION,
            profiles,
            favorites,
        })?;
        self.commit(next)
    }

    /// Adds the profiles whose id is not in the store yet, and their favorite
    /// pins (a backup imported with "add"); existing profiles stay as they
    /// are. Returns how many were added.
    pub fn add_new(&mut self, profiles: Vec<Profile>, favorites: Vec<Uuid>) -> Result<usize> {
        let mut next = self.doc.clone();
        let mut added = Vec::new();
        for p in profiles {
            if !next.profiles.iter().any(|q| q.id == p.id) {
                added.push(p.id);
                next.profiles.push(p);
            }
        }
        next.favorites
            .extend(favorites.into_iter().filter(|id| added.contains(id)));
        let next = checked(next)?;
        self.commit(next)?;
        Ok(added.len())
    }

    /// Writes `next` and only then makes it the current state: if the write
    /// fails, memory keeps what is on disk (a failed host change must not
    /// leave the old host's saved password attached to the new host).
    fn commit(&mut self, next: Document) -> Result<()> {
        self.write(&next)?;
        self.doc = next;
        Ok(())
    }

    fn write(&self, doc: &Document) -> Result<()> {
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
        serde_json::to_writer_pretty(tmp.as_file_mut(), doc)?;
        tmp.as_file_mut().write_all(b"\n")?;
        tmp.as_file_mut().sync_all()?;
        tmp.persist(&self.path).map_err(|e| Error::Io(e.error))?;
        Ok(())
    }
}

/// Checks the connections and favorites of a backup as a whole, before
/// anything is imported; returns them cleaned up like the store keeps them.
pub fn validate_backup(profiles: Vec<Profile>, favorites: Vec<Uuid>) -> Result<(Vec<Profile>, Vec<Uuid>)> {
    let doc = checked(Document {
        version: SCHEMA_VERSION,
        profiles,
        favorites,
    })?;
    Ok((doc.profiles, doc.favorites))
}

/// A whole document checked as `upsert` checks one profile: valid profiles,
/// unique ids, jump chains that exist and do not loop; favorites that point
/// nowhere or repeat are dropped.
fn checked(mut doc: Document) -> Result<Document> {
    if doc.profiles.len() > MAX_PROFILES {
        return Err(Error::InvalidProfile(format!("more than {MAX_PROFILES} connections")));
    }
    let mut ids = std::collections::HashSet::new();
    for p in &mut doc.profiles {
        p.validate()?;
        if !ids.insert(p.id) {
            return Err(Error::InvalidProfile(format!("{}: the same id is used twice", p.name)));
        }
    }
    for p in &doc.profiles {
        if let ProfileKind::Ssh(target) = &p.kind
            && target.jump.is_some()
        {
            resolve_jumps(&doc.profiles, target).map_err(|e| Error::InvalidProfile(format!("{}: {e}", p.name)))?;
        }
    }
    let mut seen = std::collections::HashSet::new();
    doc.favorites.retain(|id| ids.contains(id) && seen.insert(*id));
    Ok(doc)
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
    fn replace_and_add_are_checked_as_a_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut store = ProfileStore::open(&path).unwrap();
        let keep = store.upsert(local("keep")).unwrap();

        let a = local("a");
        let b = local("b");
        // Unknown and repeated favorites are dropped.
        store
            .replace_all(vec![a.clone(), b.clone()], vec![b.id, Uuid::new_v4(), b.id])
            .unwrap();
        assert_eq!(store.profiles().len(), 2);
        assert_eq!(store.favorites(), &[b.id]);

        // Adding keeps what is there; profiles with a known id are skipped.
        let mut changed = a.clone();
        changed.name = "renamed".into();
        let added = store.add_new(vec![changed, keep.clone()], vec![keep.id, a.id]).unwrap();
        assert_eq!(added, 1);
        assert_eq!(store.get(a.id).unwrap().name, "a");
        assert_eq!(store.favorites(), &[b.id, keep.id]);

        // A broken document changes nothing, on disk or in memory.
        let mut bad = local("bad");
        bad.name = String::new();
        assert!(store.replace_all(vec![bad], vec![]).is_err());
        assert!(store.replace_all(vec![a.clone(), a.clone()], vec![]).is_err());
        assert_eq!(store.profiles().len(), 3);
        assert_eq!(ProfileStore::open(&path).unwrap().profiles().len(), 3);
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
    fn rejects_broken_jump_chains() {
        use crate::profile::{SshAuth, SshTarget};
        let ssh = |name: &str, jump| {
            Profile::new(
                name,
                ProfileKind::Ssh(SshTarget {
                    host: format!("{name}.example"),
                    port: 22,
                    username: "u".into(),
                    auth: SshAuth::Auto,
                    keepalive_secs: 30,
                    jump,
                    forwards: vec![],
                }),
            )
        };
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProfileStore::open(dir.path().join("p.json")).unwrap();
        let a = store.upsert(ssh("a", None)).unwrap();
        let b = store.upsert(ssh("b", Some(a.id))).unwrap();
        // Missing jump host.
        assert!(store.upsert(ssh("c", Some(uuid::Uuid::new_v4()))).is_err());
        // Local shell as jump host.
        let shell = store.upsert(local("sh")).unwrap();
        assert!(store.upsert(ssh("d", Some(shell.id))).is_err());
        // a -> b -> a would loop.
        let mut a2 = a.clone();
        if let ProfileKind::Ssh(t) = &mut a2.kind {
            t.jump = Some(b.id);
        }
        assert!(store.upsert(a2).is_err());
        // A jump host cannot become a local shell.
        let mut a3 = local("a");
        a3.id = a.id;
        assert!(store.upsert(a3).is_err());
        assert!(store.is_jump_host(a.id) && !store.is_jump_host(b.id));
        assert_eq!(store.profiles().len(), 3);
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
