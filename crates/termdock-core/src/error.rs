use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid profile: {0}")]
    InvalidProfile(String),

    #[error("profile not found: {0}")]
    ProfileNotFound(uuid::Uuid),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not parse stored data: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("credential store error: {0}")]
    Vault(String),

    #[error(
        "host key for {host} has CHANGED (line {line}). This may be a man-in-the-middle attack; the connection was refused. If the server was legitimately reinstalled, verify the new fingerprint with its administrator, then use \"Forget host key\" in the connection manager"
    )]
    HostKeyChanged { host: String, line: usize },

    #[error("host key for {0} was not trusted")]
    HostKeyRejected(String),

    #[error("authentication failed for {0}")]
    AuthFailed(String),

    #[error("authentication cancelled")]
    AuthCancelled,

    #[error("SSH error: {0}")]
    Ssh(#[from] russh::Error),

    #[error("SSH key error: {0}")]
    SshKey(#[from] russh::keys::Error),

    #[error("terminal error: {0}")]
    Pty(String),

    #[error("jump host: {0}")]
    Jump(String),

    #[error("connection timed out")]
    Timeout,

    #[error("session is closed")]
    SessionClosed,

    #[error("SFTP: {0}")]
    Sftp(String),

    #[error("file transfer needs an SSH session")]
    NotSsh,

    #[error("invalid file name: {0:?}")]
    InvalidName(String),

    #[error("{0:?} already exists")]
    Exists(String),

    #[error("cancelled")]
    Cancelled,

    #[error("{0:?} is not a regular file")]
    NotRegular(String),

    #[error("sudo: {0}")]
    Sudo(String),

    #[error("the file changed size during the transfer; nothing was replaced")]
    SizeMismatch,

    #[error("the file is too large for this")]
    TooLarge,

    #[error("sftp-server was not found on the server, so files cannot be opened as another user")]
    NoSftpServer,

    #[error("more than {0} files and folders: transfer smaller folders")]
    TooManyItems(usize),
}

impl Error {
    /// A failure to connect that may go away by itself (network down, server
    /// restarting, a jump host that cannot reach it yet), as opposed to one
    /// that needs the user (authentication, host key, a missing profile).
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Ssh(_) | Self::Timeout | Self::Jump(_))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
