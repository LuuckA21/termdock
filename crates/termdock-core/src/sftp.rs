//! SFTP over an open SSH terminal session: the `sftp` subsystem runs on new
//! channels of the same, already authenticated connection.
//!
//! The server is not trusted. Its names never become local paths here; a
//! listing is read in bounded steps with a deadline; downloads accept only
//! regular files, read at most the size reported when opened, go to a
//! temporary file next to the destination the user picked in a native dialog
//! (the caller's job) and are moved into place only when complete. Uploads go
//! to a randomly named temporary file created exclusively with mode 0600 and
//! replace the target only once complete, keeping its permissions.

use std::path::Path;
use std::time::Duration;

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::{RawSftpSession, SftpSession};
pub use russh_sftp::protocol::FileType;
use russh_sftp::protocol::{FileAttributes, OpenFlags, Packet, StatusCode};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub use tokio_util::sync::CancellationToken;

use russh::ChannelMsg;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::session::SessionHandle;
use crate::ssh::SshHandle;

/// Directory listings stop after this many entries (the UI says so).
pub const MAX_ENTRIES: usize = 20_000;
/// Longest file name accepted for new names (most file systems allow 255 bytes).
const MAX_NAME_LEN: usize = 255;
const CHUNK: usize = 256 * 1024;
/// Server error messages are shown to the user: keep them short.
const MAX_MESSAGE_LEN: usize = 300;
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// A whole listing must arrive within this time, however the server paces it.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const POSIX_RENAME: &str = "posix-rename@openssh.com";

fn short(text: String) -> String {
    text.chars().take(MAX_MESSAGE_LEN).collect()
}

impl From<SftpError> for Error {
    fn from(e: SftpError) -> Self {
        match e {
            SftpError::Status(s) if !s.error_message.trim().is_empty() => Error::Sftp(short(s.error_message)),
            other => Error::Sftp(short(other.to_string())),
        }
    }
}

/// I/O errors of remote files carry text from the server.
fn remote_io(e: std::io::Error) -> Error {
    Error::Sftp(short(e.to_string()))
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

/// Invisible characters that change how a name reads (Unicode "format"
/// characters: bidi overrides, zero-width marks, BOM, tags…).
pub fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00ad}' | '\u{0600}'..='\u{0605}' | '\u{061c}' | '\u{06dd}' | '\u{070f}' | '\u{180e}'
        | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}'
        | '\u{e0001}' | '\u{e0020}'..='\u{e007f}')
}

/// One path component: not empty, not `.`/`..`, no `/` or NUL, and no U+FFFD
/// (a name the server sent in another encoding cannot be named back safely).
fn check_component(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0', '\u{fffd}']) {
        Err(Error::InvalidName(name.chars().take(64).collect()))
    } else {
        Ok(())
    }
}

/// A new name typed by the user (new folder, rename, upload target): a single
/// component, at most 255 bytes, without control or invisible characters.
pub fn validate_name(name: &str) -> Result<()> {
    check_component(name)?;
    if name.len() > MAX_NAME_LEN || name.chars().any(|c| c.is_control() || is_format_char(c)) {
        return Err(Error::InvalidName(name.chars().take(64).collect()));
    }
    Ok(())
}

/// Joins a remote directory and a checked name (SFTP paths are POSIX).
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// `name` cut to at most `max` bytes on a character boundary.
fn truncate_bytes(name: &str, max: usize) -> &str {
    let mut end = name.len().min(max);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// Hidden, unpredictable sibling name used while uploading or replacing.
fn scratch_name(name: &str, tag: &str) -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    format!(".{}.{}.{tag}", truncate_bytes(name, 200), &random[..12])
}

fn kind_of(t: FileType) -> EntryKind {
    match t {
        FileType::Dir => EntryKind::Dir,
        FileType::File => EntryKind::File,
        FileType::Symlink => EntryKind::Link,
        FileType::Other => EntryKind::Other,
    }
}

fn ssh_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Runs `fut` unless `cancel` fires first.
async fn cancellable<T>(cancel: &CancellationToken, fut: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Cancelled),
        r = fut => r,
    }
}

pub struct Sftp {
    /// File transfers (pipelined reads and writes).
    inner: SftpSession,
    /// Second channel for what `SftpSession` does not expose: bounded
    /// directory reads and the atomic `posix-rename` extension.
    raw: RawSftpSession,
    posix_rename: bool,
}

/// Where OpenSSH installs `sftp-server` (Debian/Ubuntu, RHEL/Fedora, Arch,
/// macOS, BSD, Alpine, NixOS…). Only these fixed paths are ever run through
/// sudo; the server's reply to the probe is only compared with this list.
const SFTP_SERVERS: &[&str] = &[
    "/usr/lib/openssh/sftp-server",
    "/usr/libexec/openssh/sftp-server",
    "/usr/lib/ssh/sftp-server",
    "/usr/libexec/sftp-server",
    "/usr/lib/sftp-server",
    "/usr/local/libexec/sftp-server",
    "/run/current-system/sw/libexec/sftp-server",
];
/// sudo's own output kept while waiting for its prompt or for sftp-server.
const MAX_SUDO_OUTPUT: usize = 64 * 1024;
/// What `sftp-server -e -l INFO` prints once it runs, i.e. sudo is done.
const SFTP_STARTED: &str = "session opened for local user";

type SshChannel = russh::Channel<russh::client::Msg>;
type SshStream = russh::ChannelStream<russh::client::Msg>;

/// A Unix user name to run SFTP as: letters, digits, `_`, `.`, `-`, starting
/// with a letter or `_`, at most 32 characters. Strict on purpose: it is
/// placed in a remote command line.
pub fn validate_user(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let ok = name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidName(name.chars().take(64).collect()))
    }
}

/// Opens a channel for `command`; fails if the server refuses to run it.
async fn exec_channel(handle: &SshHandle, command: &str) -> Result<SshChannel> {
    let channel = handle.channel_open_session().await?;
    if let Err(e) = channel.exec(true, command.as_bytes()).await {
        let _ = channel.close().await;
        return Err(e.into());
    }
    Ok(channel)
}

/// Finds `sftp-server` among [`SFTP_SERVERS`] on the server.
async fn find_sftp_server(handle: &SshHandle) -> Result<&'static str> {
    // No quotes needed (and none used): the paths have no spaces.
    let probe = format!(
        "sh -c 'for p in {}; do if [ -x $p ]; then echo $p; exit 0; fi; done; exit 1'",
        SFTP_SERVERS.join(" ")
    );
    let mut channel = exec_channel(handle, &probe).await?;
    let mut out = Vec::new();
    let read = async {
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } if out.len() < MAX_SUDO_OUTPUT => out.extend_from_slice(&data),
                ChannelMsg::Failure => return Err(Error::Sudo("the server refused to run commands".into())),
                ChannelMsg::Close | ChannelMsg::Eof => break,
                _ => {}
            }
        }
        Ok(())
    };
    let result = tokio::time::timeout(OPEN_TIMEOUT, read).await;
    let _ = channel.close().await;
    result.map_err(|_| Error::Timeout)??;
    // Login scripts may print first: the path is the last line.
    let text = String::from_utf8_lossy(&out);
    let last = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default();
    SFTP_SERVERS
        .iter()
        .copied()
        .find(|p| *p == last)
        .ok_or(Error::NoSftpServer)
}

/// The line of sudo's output that explains a failure, cleaned for display:
/// the last one naming sudo, else the last one (the first may be the lecture).
fn sudo_message(stderr: &str, marker: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains(marker))
        .collect();
    let line = lines
        .iter()
        .rev()
        .find(|l| l.starts_with("sudo"))
        .or(lines.last())
        .copied()
        .unwrap_or("sudo did not start the file server");
    short(
        line.chars()
            .filter(|c| !c.is_control() && !is_format_char(*c))
            .collect(),
    )
}

/// Where sudo got to on a channel.
enum Step {
    /// sftp-server runs: the channel carries SFTP from now on.
    Started,
    /// sudo printed our prompt and waits for a password, for this user
    /// (`%p`: the login user, or root/the target with `rootpw`/`targetpw`).
    Prompted(Option<String>),
}

/// One `sudo … sftp-server` on its own channel, before SFTP starts.
struct Waiting {
    channel: SshChannel,
    /// Random, per channel: only sudo's real prompt contains it.
    marker: String,
    stderr: Vec<u8>,
    /// sudo printed the prompt and waits for input.
    prompted: bool,
    asks_for: Option<String>,
    /// A password was already given to this sudo (a new prompt means retry).
    answered: bool,
}

impl Waiting {
    async fn start(handle: &SshHandle, server: &str, user: &str) -> Result<Self> {
        let marker = format!("TDK{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
        // `--prompt`: sudo writes it (to stderr, with -S) exactly when it wants
        // a password; `%p` is whose. `-e -l INFO`: sftp-server says when it
        // runs. `-d %d`: start in that user's home. LC_ALL=C: English text.
        let command =
            format!("sh -c 'LC_ALL=C exec sudo -S --prompt={marker}/%p/ -u {user} -- {server} -e -l INFO -d %d'");
        let channel = exec_channel(handle, &command).await?;
        Ok(Self {
            channel,
            marker,
            stderr: Vec::new(),
            prompted: false,
            asks_for: None,
            answered: false,
        })
    }

    /// Reads sudo's output until sftp-server runs or sudo prompts.
    async fn advance(&mut self) -> Result<Step> {
        let read = async {
            while let Some(msg) = self.channel.wait().await {
                match msg {
                    ChannelMsg::ExtendedData { data, .. } => {
                        if self.stderr.len() < MAX_SUDO_OUTPUT {
                            self.stderr.extend_from_slice(&data);
                        }
                        let text = String::from_utf8_lossy(&self.stderr).into_owned();
                        if text.contains(SFTP_STARTED) {
                            return Ok(Step::Started);
                        }
                        let open = format!("{}/", self.marker);
                        if let Some(at) = text.find(&open) {
                            let rest = &text[at + open.len()..];
                            let Some(close) = rest.find('/') else { continue };
                            let who = &rest[..close];
                            let who = validate_user(who).is_ok().then(|| who.to_owned());
                            // Keep what came before the prompt (lecture,
                            // "Sorry, try again"), drop the prompt itself.
                            let kept = text.as_bytes()[..at].to_vec();
                            self.stderr = kept;
                            return Ok(Step::Prompted(who));
                        }
                    }
                    ChannelMsg::Failure => {
                        return Err(Error::Sudo("the server refused to run sudo".into()));
                    }
                    ChannelMsg::ExitStatus { .. } | ChannelMsg::Eof | ChannelMsg::Close => {
                        let text = String::from_utf8_lossy(&self.stderr).into_owned();
                        return Err(Error::Sudo(sudo_message(&text, &self.marker)));
                    }
                    _ => {}
                }
            }
            Err(Error::SessionClosed)
        };
        tokio::time::timeout(OPEN_TIMEOUT, read)
            .await
            .map_err(|_| Error::Timeout)?
    }

    async fn close(self) {
        let _ = self.channel.close().await;
    }
}

/// Opening a file browser through sudo, kept between password attempts: the
/// same sudo processes receive the retries, like typing in a terminal (sudo
/// itself allows three).
pub struct SudoPending {
    handle: SshHandle,
    server: &'static str,
    user: String,
    /// Channels where sftp-server already runs (two are needed).
    ready: Vec<SshStream>,
    waiting: Option<Waiting>,
}

impl SudoPending {
    /// Whose password sudo asks for, when it says.
    pub fn asks_for(&self) -> Option<&str> {
        self.waiting.as_ref().and_then(|w| w.asks_for.as_deref())
    }

    /// Stops the attempt: sudo gets end-of-file, channels close.
    pub async fn cancel(mut self) {
        if let Some(w) = self.waiting.take() {
            w.close().await;
        }
    }

    /// Moves on: starts the channels, answers prompts with `password` (each
    /// sudo gets it at most once per call), and stops at the first prompt it
    /// cannot answer.
    async fn drive(mut self, password: Option<&str>) -> Result<OpenAs> {
        loop {
            if self.ready.len() == 2 {
                let raw = RawSftpSession::new(self.ready.pop().expect("two"));
                let inner = SftpSession::new(self.ready.pop().expect("one")).await?;
                return Ok(OpenAs::Opened(Box::new(Sftp::assemble(inner, raw).await?)));
            }
            let mut w = match self.waiting.take() {
                Some(w) => w,
                None => Waiting::start(&self.handle, self.server, &self.user).await?,
            };
            if w.prompted {
                let Some(pw) = password else {
                    let retry = w.answered;
                    self.waiting = Some(w);
                    return Ok(OpenAs::PasswordRequired { pending: self, retry });
                };
                let mut line = Zeroizing::new(Vec::with_capacity(pw.len() + 1));
                line.extend_from_slice(pw.as_bytes());
                line.push(b'\n');
                if let Err(e) = w.channel.data(&line[..]).await {
                    w.close().await;
                    return Err(e.into());
                }
                w.prompted = false;
                w.answered = true;
            }
            match w.advance().await {
                Ok(Step::Started) => self.ready.push(w.channel.into_stream()),
                Ok(Step::Prompted(who)) => {
                    w.prompted = true;
                    w.asks_for = who;
                    let rejected = w.answered;
                    self.waiting = Some(w);
                    // Our password was not accepted (or sudo wants something
                    // else, e.g. a one-time code), or none was given: ask.
                    if rejected || password.is_none() {
                        return Ok(OpenAs::PasswordRequired {
                            pending: self,
                            retry: rejected,
                        });
                    }
                }
                Err(e) => {
                    w.close().await;
                    return Err(e);
                }
            }
        }
    }
}

impl Drop for SudoPending {
    /// A forgotten attempt still closes its channel (sudo gets end-of-file).
    fn drop(&mut self) {
        if let Some(w) = self.waiting.take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            rt.spawn(w.close());
        }
    }
}

/// Result of opening a file browser as another user.
pub enum OpenAs {
    Opened(Box<Sftp>),
    /// sudo waits for a password (`retry`: the one given was not accepted).
    PasswordRequired {
        pending: SudoPending,
        retry: bool,
    },
}

impl Sftp {
    /// Starts the `sftp` subsystem on the session's SSH connection.
    pub async fn open(session: &SessionHandle) -> Result<Self> {
        let handle = session.ssh().ok_or(Error::NotSsh)?;
        let opened = async {
            let start = || async {
                let channel = handle.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                Ok::<_, Error>(channel.into_stream())
            };
            let inner = SftpSession::new(start().await?).await?;
            let raw = RawSftpSession::new(start().await?);
            Self::assemble(inner, raw).await
        };
        tokio::time::timeout(OPEN_TIMEOUT, opened)
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Starts `sftp-server` as `user` through `sudo` on the session's SSH
    /// connection, like `sudo -iu user` in the terminal. Needs sudo rights for
    /// it on the server. A password is only sent when sudo prints its prompt
    /// (see [`Sftp::continue_as`]); it is never stored.
    pub async fn open_as(session: &SessionHandle, user: &str) -> Result<OpenAs> {
        validate_user(user)?;
        let handle = session.ssh().ok_or(Error::NotSsh)?;
        if session.ssh_user() == Some(user) {
            return Ok(OpenAs::Opened(Box::new(Self::open(session).await?)));
        }
        let server = find_sftp_server(handle).await?;
        SudoPending {
            handle: handle.clone(),
            server,
            user: user.to_owned(),
            ready: Vec::new(),
            waiting: None,
        }
        .drive(None)
        .await
    }

    /// Gives `password` to the sudo that is waiting for it.
    pub async fn continue_as(pending: SudoPending, password: &str) -> Result<OpenAs> {
        pending.drive(Some(password)).await
    }

    async fn assemble(inner: SftpSession, raw: RawSftpSession) -> Result<Self> {
        let version = raw.init().await?;
        let posix_rename = version.extensions.get(POSIX_RENAME).is_some_and(|v| v == "1");
        Ok(Self {
            inner,
            raw,
            posix_rename,
        })
    }

    /// The canonical absolute form of `path` (`"."` is the login directory).
    pub async fn canonicalize(&self, path: &str) -> Result<String> {
        Ok(self.inner.canonicalize(path).await?)
    }

    /// Lists a directory: folders first, then by name; `.` and `..` omitted.
    /// Reading stops at [`MAX_ENTRIES`] or after [`LIST_TIMEOUT`].
    pub async fn list(&self, path: &str) -> Result<Listing> {
        let path = self.canonicalize(path).await?;
        let handle = self.raw.opendir(path.as_str()).await?.handle;
        let read = async {
            let mut entries = Vec::new();
            loop {
                let batch = match self.raw.readdir(handle.as_str()).await {
                    Ok(batch) => batch,
                    Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => return Ok((entries, false)),
                    Err(e) => return Err(Error::from(e)),
                };
                for f in batch.files {
                    if f.filename == "." || f.filename == ".." {
                        continue;
                    }
                    if entries.len() == MAX_ENTRIES {
                        return Ok((entries, true));
                    }
                    entries.push(Entry {
                        kind: kind_of(f.attrs.file_type()),
                        size: f.attrs.size,
                        modified: f.attrs.mtime,
                        mode: f.attrs.permissions.map(|p| p & 0o7777),
                        name: f.filename,
                    });
                }
            }
        };
        let result = tokio::time::timeout(LIST_TIMEOUT, read).await;
        let _ = self.raw.close(handle).await;
        let (mut entries, truncated) =
            result.map_err(|_| Error::Sftp("the server took too long to list the folder".into()))??;
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

    /// Renames within `dir`; never replaces an existing name (SFTP v3 rename).
    pub async fn rename(&self, dir: &str, from: &str, to: &str) -> Result<()> {
        check_component(from)?;
        validate_name(to)?;
        Ok(self.inner.rename(join(dir, from), join(dir, to)).await?)
    }

    /// Size and modification time (seconds) of the file at `dir/name`.
    pub async fn file_info(&self, dir: &str, name: &str) -> Result<(u64, Option<u32>, FileType)> {
        check_component(name)?;
        let meta = self.inner.metadata(join(dir, name)).await?;
        Ok((meta.size.unwrap_or(0), meta.mtime, meta.file_type()))
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

    /// Downloads the regular file `dir/name` to `local` (a path the user
    /// picked). Reads at most the size the server reported when the file was
    /// opened. `progress(done, total)` is called as data arrives.
    pub async fn download(
        &self,
        dir: &str,
        name: &str,
        local: &Path,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        check_component(name)?;
        let remote = join(dir, name);
        let (mut src, total) = cancellable(cancel, async {
            let src = self.inner.open(remote).await?;
            // fstat of what was opened: no devices, FIFOs or other surprises.
            let meta = src.metadata().await?;
            if meta.file_type() != FileType::File {
                return Err(Error::NotRegular(name.chars().take(64).collect()));
            }
            Ok((src, meta.size))
        })
        .await?;

        let dir = local
            .parent()
            .ok_or_else(|| Error::InvalidName(local.display().to_string()))?;
        // Random name, created exclusively, 0600 on Unix; removed unless persisted.
        let tmp = tempfile::Builder::new()
            .prefix(".termdock-")
            .suffix(".part")
            .tempfile_in(dir)?;
        let mut out = tokio::fs::File::from_std(tmp.as_file().try_clone()?);
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        loop {
            let want = match total {
                Some(t) if done >= t => break,
                Some(t) => CHUNK.min((t - done) as usize),
                None => CHUNK,
            };
            let n = cancellable(cancel, async { src.read(&mut buf[..want]).await.map_err(remote_io) }).await?;
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

    /// Uploads `local` (already open, a regular file the user picked) as
    /// `dir/name`; see [`Sftp::upload_from`].
    #[allow(clippy::too_many_arguments)]
    pub async fn upload(
        &self,
        local: std::fs::File,
        local_mode: Option<u32>,
        dir: &str,
        name: &str,
        overwrite: bool,
        cancel: &CancellationToken,
        progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let src = tokio::fs::File::from_std(local);
        let size = src.metadata().await?.len();
        self.upload_from(src, size, local_mode, dir, name, overwrite, cancel, progress)
            .await
    }

    /// Uploads exactly `size` bytes read from `src` as `dir/name`. An existing
    /// file is only replaced when `overwrite` is set, after the upload
    /// completed, keeping that file's permissions; a new file gets
    /// `local_mode` (or 0644). Fewer or more bytes than `size` abort it.
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_from<R: tokio::io::AsyncRead + Unpin>(
        &self,
        mut src: R,
        size: u64,
        local_mode: Option<u32>,
        dir: &str,
        name: &str,
        overwrite: bool,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        validate_name(name)?;
        let target = join(dir, name);
        let existing = match self.inner.symlink_metadata(target.as_str()).await {
            Ok(meta) => Some(meta),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => None,
            Err(e) => return Err(e.into()),
        };
        if existing.is_some() && !overwrite {
            return Err(Error::Exists(name.to_owned()));
        }
        if existing.as_ref().is_some_and(|m| m.file_type() == FileType::Dir) {
            return Err(Error::Exists(name.to_owned()));
        }
        let total = Some(size);

        // Exclusive create with a random name: a file or link planted at a
        // predictable name cannot redirect the write.
        let tmp = join(dir, &scratch_name(name, "upload"));
        let private = FileAttributes {
            permissions: Some(0o600),
            ..FileAttributes::default()
        };
        let mut dst = self
            .inner
            .open_with_flags_and_attributes(
                tmp.as_str(),
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                private,
            )
            .await?;

        let copied = cancellable(cancel, async {
            let mut buf = vec![0u8; CHUNK];
            let mut done = 0u64;
            loop {
                let n = src.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                done += n as u64;
                if done > size {
                    return Err(Error::SizeMismatch);
                }
                dst.write_all(&buf[..n]).await.map_err(remote_io)?;
                progress(done, total);
            }
            if done != size {
                return Err(Error::SizeMismatch);
            }
            // fsync@openssh.com when the server has it.
            let _ = dst.sync_all().await;
            dst.shutdown().await.map_err(remote_io)?;
            Ok(done)
        })
        .await;
        let done = match copied {
            Ok(done) => done,
            Err(e) => {
                let _ = self.inner.remove_file(tmp).await;
                return Err(e);
            }
        };

        // Keep the replaced file's permissions (for a link, those of its target).
        let existing_mode = match &existing {
            Some(m) if m.file_type() == FileType::Symlink => self
                .inner
                .metadata(target.as_str())
                .await
                .ok()
                .and_then(|t| t.permissions),
            Some(m) => m.permissions,
            None => None,
        };
        let mode = existing_mode.or(local_mode).unwrap_or(0o644) & 0o7777;
        let attrs = FileAttributes {
            permissions: Some(mode),
            ..FileAttributes::default()
        };
        let placed = async {
            self.inner.set_metadata(tmp.as_str(), attrs).await?;
            self.replace(&tmp, &target, existing.is_some()).await
        };
        if let Err(e) = placed.await {
            let _ = self.inner.remove_file(tmp).await;
            return Err(e);
        }
        Ok(done)
    }

    /// Moves `tmp` onto `target`. With `posix-rename` the swap is atomic;
    /// otherwise the old file is set aside first and put back on failure, so
    /// it is never lost.
    async fn replace(&self, tmp: &str, target: &str, exists: bool) -> Result<()> {
        if !exists {
            // Plain SFTP rename fails if the name appeared meanwhile: nothing
            // the user did not agree to is replaced.
            return Ok(self.inner.rename(tmp, target).await?);
        }
        if self.posix_rename {
            let mut data = Vec::new();
            ssh_string(&mut data, tmp);
            ssh_string(&mut data, target);
            return match self.raw.extended(POSIX_RENAME, data).await? {
                Packet::Status(s) if s.status_code == StatusCode::Ok => Ok(()),
                Packet::Status(s) => Err(SftpError::Status(s).into()),
                _ => Err(Error::Sftp("unexpected reply to posix-rename".into())),
            };
        }
        let (dir, name) = target.rsplit_once('/').unwrap_or(("", target));
        let backup = join(if dir.is_empty() { "/" } else { dir }, &scratch_name(name, "old"));
        self.inner.rename(target, backup.as_str()).await?;
        match self.inner.rename(tmp, target).await {
            Ok(()) => {
                let _ = self.inner.remove_file(backup).await;
                Ok(())
            }
            Err(e) => {
                let _ = self.inner.rename(backup, target).await;
                Err(e.into())
            }
        }
    }

    pub async fn close(&self) {
        let _ = self.inner.close().await;
        let _ = self.raw.close_session();
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
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "/abs",
            "nul\0",
            "line\nbreak",
            "evil\u{202e}gpj.exe",
            "zero\u{200b}width",
            "bad\u{fffd}",
            &"x".repeat(256),
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn users() {
        for ok in ["bob", "_svc", "www-data", "a.b", "u1", &"x".repeat(32)] {
            assert!(validate_user(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-u",
            "1abc",
            ".x",
            "a b",
            "x;id",
            "$(id)",
            "a'b",
            "a\"b",
            "é",
            &"x".repeat(33),
        ] {
            assert!(validate_user(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sudo_messages() {
        let lecture = "We trust you have received the usual lecture.\n\nTDKabc/bob/\nSorry, try again.\nsudo: 3 incorrect password attempts\n";
        assert_eq!(sudo_message(lecture, "TDKabc"), "sudo: 3 incorrect password attempts");
        assert_eq!(sudo_message("odd failure\n", "TDK"), "odd failure");
        assert_eq!(sudo_message("", "TDK"), "sudo did not start the file server");
        assert_eq!(sudo_message("sudo: \u{202e}bad\x07\n", "TDK"), "sudo: bad");
    }

    #[test]
    fn joins() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/home/u", "f"), "/home/u/f");
        assert_eq!(join("/home/u/", "f"), "/home/u/f");
    }

    #[test]
    fn scratch_names_stay_short() {
        let long = "é".repeat(127); // 254 bytes
        let s = scratch_name(&long, "upload");
        assert!(s.len() <= 255, "{}", s.len());
        assert!(s.starts_with('.'));
        assert_ne!(scratch_name("a", "upload"), scratch_name("a", "upload"));
    }
}
