//! SFTP over an open SSH terminal session: the `sftp` subsystem runs on a new
//! channel of the same, already authenticated connection.
//!
//! Remote names come from the server and are untrusted: they never become
//! local paths here. Downloads go to a path the user picked in a native save
//! dialog (the caller's job), written to a temporary file next to it and moved
//! into place only when complete. Uploads go to a hidden temporary name and
//! replace the target only once fully written.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{FileType, OpenFlags};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{Error, Result};
use crate::session::SessionHandle;

/// Directory listings are cut at this many entries (the UI says so).
pub const MAX_ENTRIES: usize = 20_000;
/// Longest file name accepted for new names (most file systems allow 255 bytes).
const MAX_NAME_LEN: usize = 255;
const CHUNK: usize = 256 * 1024;
/// Server error messages are shown to the user: keep them short.
const MAX_MESSAGE_LEN: usize = 300;

impl From<SftpError> for Error {
    fn from(e: SftpError) -> Self {
        let text = match e {
            SftpError::Status(s) if !s.error_message.trim().is_empty() => s.error_message,
            other => other.to_string(),
        };
        Error::Sftp(text.chars().take(MAX_MESSAGE_LEN).collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Dir,
    File,
    Link,
    Other,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    /// Seconds since the Unix epoch.
    pub modified: Option<u32>,
    /// Unix permission bits (`0o7777` mask).
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// Canonical absolute path of the directory.
    pub path: String,
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

/// Set to stop a running transfer; the partial file is removed.
pub type CancelFlag = Arc<AtomicBool>;

/// One path component: not empty, not `.`/`..`, no `/` or NUL. Applied to
/// names coming back from a listing before they are joined to a directory.
fn check_component(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        Err(Error::InvalidName(name.chars().take(64).collect()))
    } else {
        Ok(())
    }
}

/// A new name typed by the user (new folder, rename, upload target): a single
/// component, at most 255 bytes, without control characters.
pub fn validate_name(name: &str) -> Result<()> {
    check_component(name)?;
    if name.len() > MAX_NAME_LEN || name.chars().any(char::is_control) {
        return Err(Error::InvalidName(name.chars().take(64).collect()));
    }
    Ok(())
}

/// Joins a remote directory and a validated name (SFTP paths are POSIX).
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn kind_of(t: FileType) -> EntryKind {
    match t {
        FileType::Dir => EntryKind::Dir,
        FileType::File => EntryKind::File,
        FileType::Symlink => EntryKind::Link,
        FileType::Other => EntryKind::Other,
    }
}

pub struct Sftp {
    inner: SftpSession,
}

impl Sftp {
    /// Starts the `sftp` subsystem on the session's SSH connection.
    pub async fn open(session: &SessionHandle) -> Result<Self> {
        let handle = session.ssh().ok_or(Error::NotSsh)?;
        let channel = handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let inner = SftpSession::new(channel.into_stream()).await?;
        Ok(Self { inner })
    }

    /// The canonical absolute form of `path` (`"."` is the login directory).
    pub async fn canonicalize(&self, path: &str) -> Result<String> {
        Ok(self.inner.canonicalize(path).await?)
    }

    /// Lists a directory: folders first, then by name; `.` and `..` omitted.
    pub async fn list(&self, path: &str) -> Result<Listing> {
        let path = self.canonicalize(path).await?;
        let mut entries = Vec::new();
        let mut truncated = false;
        for e in self.inner.read_dir(path.as_str()).await? {
            let name = e.file_name();
            if name == "." || name == ".." {
                continue;
            }
            if entries.len() == MAX_ENTRIES {
                truncated = true;
                break;
            }
            let meta = e.metadata();
            entries.push(Entry {
                name,
                kind: kind_of(meta.file_type()),
                size: meta.size,
                modified: meta.mtime,
                mode: meta.permissions.map(|p| p & 0o7777),
            });
        }
        entries.sort_by(|a, b| {
            (a.kind != EntryKind::Dir)
                .cmp(&(b.kind != EntryKind::Dir))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(Listing {
            path,
            entries,
            truncated,
        })
    }

    pub async fn mkdir(&self, dir: &str, name: &str) -> Result<()> {
        validate_name(name)?;
        Ok(self.inner.create_dir(join(dir, name)).await?)
    }

    pub async fn rename(&self, dir: &str, from: &str, to: &str) -> Result<()> {
        check_component(from)?;
        validate_name(to)?;
        Ok(self.inner.rename(join(dir, from), join(dir, to)).await?)
    }

    /// Removes a file, a link or an *empty* directory (never recursive).
    pub async fn remove(&self, dir: &str, name: &str) -> Result<()> {
        check_component(name)?;
        let path = join(dir, name);
        // lstat: a link to a directory is removed as a link.
        if self.inner.symlink_metadata(path.as_str()).await?.file_type() == FileType::Dir {
            Ok(self.inner.remove_dir(path).await?)
        } else {
            Ok(self.inner.remove_file(path).await?)
        }
    }

    pub async fn exists(&self, dir: &str, name: &str) -> Result<bool> {
        Ok(self.inner.try_exists(join(dir, name)).await?)
    }

    /// Downloads `remote` to `local`. `progress(done, total)` is called as
    /// data arrives. Returns the number of bytes written.
    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        cancel: &CancelFlag,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let mut src = self.inner.open(remote).await?;
        let total = src.metadata().await.ok().and_then(|m| m.size);
        let dir = local
            .parent()
            .ok_or_else(|| Error::InvalidName(local.display().to_string()))?;
        // Created 0600 on Unix; removed automatically unless persisted.
        let tmp = tempfile::Builder::new()
            .prefix(".termdock-")
            .suffix(".part")
            .tempfile_in(dir)?;
        let mut out = tokio::fs::File::from_std(tmp.reopen()?);
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).await?;
            done += n as u64;
            progress(done, total);
        }
        out.flush().await?;
        out.sync_all().await?;
        drop(out);
        let _ = src.shutdown().await;
        tmp.persist(local).map_err(|e| Error::Io(e.error))?;
        mark_downloaded(local);
        Ok(done)
    }

    /// Uploads `local` as `dir/name`. An existing file of that name is only
    /// replaced when `overwrite` is set, and only after the upload completed.
    pub async fn upload(
        &self,
        local: &Path,
        dir: &str,
        name: &str,
        overwrite: bool,
        cancel: &CancelFlag,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        validate_name(name)?;
        let target = join(dir, name);
        if !overwrite && self.inner.try_exists(target.as_str()).await? {
            return Err(Error::Exists(name.to_owned()));
        }
        let mut src = tokio::fs::File::open(local).await?;
        let meta = src.metadata().await?;
        if !meta.is_file() {
            return Err(Error::InvalidName(local.display().to_string()));
        }
        let total = Some(meta.len());

        let tmp_name = format!(".{}.termdock-upload", name.chars().take(200).collect::<String>());
        let tmp = join(dir, &tmp_name);
        let flags = OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE;
        let mut dst = self.inner.open_with_flags(tmp.as_str(), flags).await?;

        let copied = async {
            let mut buf = vec![0u8; CHUNK];
            let mut done = 0u64;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                let n = src.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n]).await?;
                done += n as u64;
                progress(done, total);
            }
            dst.shutdown().await?;
            Ok(done)
        }
        .await;

        let done = match copied {
            Ok(done) => done,
            Err(e) => {
                let _ = self.inner.remove_file(tmp).await;
                return Err(e);
            }
        };
        // SFTP v3 rename does not replace: remove the old file first.
        if overwrite
            && self.inner.try_exists(target.as_str()).await?
            && let Err(e) = self.inner.remove_file(target.as_str()).await
        {
            let _ = self.inner.remove_file(tmp).await;
            return Err(e.into());
        }
        if let Err(e) = self.inner.rename(tmp.as_str(), target).await {
            let _ = self.inner.remove_file(tmp).await;
            return Err(e.into());
        }
        Ok(done)
    }

    pub async fn close(&self) {
        let _ = self.inner.close().await;
    }
}

/// Marks a downloaded file as coming from another computer, like browsers do,
/// so Windows asks before running it (Mark of the Web). Best effort: file
/// systems without alternate data streams just ignore it.
#[cfg(windows)]
fn mark_downloaded(path: &Path) {
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let _ = std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
}

#[cfg(not(windows))]
fn mark_downloaded(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a", "file.txt", ".hidden", "with space", "ünïcode", "..dots"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "/abs", "nul\0", "line\nbreak", &"x".repeat(256)] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn joins() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/home/u", "f"), "/home/u/f");
        assert_eq!(join("/home/u/", "f"), "/home/u/f");
    }
}
