//! Connection profiles: what the user saves in the connection manager and pins
//! to the favorites bar. Profiles never contain secrets; passwords and key
//! passphrases live in the OS credential vault (see [`crate::vault`]).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};

pub const DEFAULT_SSH_PORT: u16 = 22;
const MAX_NAME_LEN: usize = 64;
const MAX_HOST_LEN: usize = 253;
const MAX_USER_LEN: usize = 128;
const MAX_GROUP_LEN: usize = 64;
const MAX_ARGS: usize = 64;
const MAX_ARG_LEN: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    #[serde(flatten)]
    pub kind: ProfileKind,
    /// Accent colour shown on the tab and the favorites bar, `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Free-form folder name used to group profiles in the manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProfileKind {
    Ssh(SshTarget),
    Local(LocalShell),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTarget {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub auth: SshAuth,
    /// Send a keepalive after this many idle seconds (0 disables it).
    #[serde(default = "default_keepalive")]
    pub keepalive_secs: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum SshAuth {
    /// SSH agent first, then the default keys in `~/.ssh`, then a password prompt.
    #[default]
    Auto,
    /// Only the SSH agent (ssh-agent, Windows OpenSSH agent or Pageant).
    Agent,
    /// A private key file, optionally passphrase protected.
    Key { path: PathBuf },
    /// Password (prompted, optionally remembered in the OS vault).
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalShell {
    /// Executable to launch, e.g. `pwsh.exe`, `/bin/zsh`.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

fn default_port() -> u16 {
    DEFAULT_SSH_PORT
}

fn default_keepalive() -> u32 {
    30
}

impl Profile {
    pub fn new(name: impl Into<String>, kind: ProfileKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind,
            color: None,
            group: None,
        }
    }

    /// Validates and normalises user supplied data. Called on every save so
    /// nothing malformed reaches the store, the vault or the network layer.
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_owned();
        check_text("name", &self.name, 1, MAX_NAME_LEN)?;

        if let Some(color) = &self.color {
            let ok = color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit());
            if !ok {
                return Err(Error::InvalidProfile("color must be #rrggbb".into()));
            }
        }
        if let Some(group) = &mut self.group {
            *group = group.trim().to_owned();
            if group.is_empty() {
                self.group = None;
            } else {
                check_text("group", group, 1, MAX_GROUP_LEN)?;
            }
        }

        match &mut self.kind {
            ProfileKind::Ssh(t) => t.validate(),
            ProfileKind::Local(l) => l.validate(),
        }
    }
}

impl SshTarget {
    fn validate(&mut self) -> Result<()> {
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        validate_host(&self.host)?;
        check_token("username", &self.username, 1, MAX_USER_LEN)?;
        if self.port == 0 {
            return Err(Error::InvalidProfile("port must be 1-65535".into()));
        }
        if let SshAuth::Key { path } = &self.auth
            && path.as_os_str().is_empty()
        {
            return Err(Error::InvalidProfile("key path is empty".into()));
        }
        if self.keepalive_secs > 3600 {
            return Err(Error::InvalidProfile("keepalive must be at most 3600 s".into()));
        }
        Ok(())
    }

    /// `user@host` or `user@[host]:port`, as shown in the UI.
    pub fn display(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == DEFAULT_SSH_PORT {
            format!("{}@{}", self.username, host)
        } else if self.host.contains(':') {
            format!("{}@{}:{}", self.username, host, self.port)
        } else {
            format!("{}@{}:{}", self.username, self.host, self.port)
        }
    }
}

impl LocalShell {
    fn validate(&mut self) -> Result<()> {
        self.program = self.program.trim().to_owned();
        check_text("program", &self.program, 1, MAX_ARG_LEN)?;
        if self.args.len() > MAX_ARGS {
            return Err(Error::InvalidProfile("too many arguments".into()));
        }
        for arg in &self.args {
            if arg.len() > MAX_ARG_LEN || arg.contains('\0') {
                return Err(Error::InvalidProfile("invalid argument".into()));
            }
        }
        if let Some(cwd) = &self.cwd
            && cwd.as_os_str().is_empty()
        {
            self.cwd = None;
        }
        Ok(())
    }
}

/// Printable text without control characters.
fn check_text(field: &str, value: &str, min: usize, max: usize) -> Result<()> {
    let len = value.chars().count();
    if len < min || len > max {
        return Err(Error::InvalidProfile(format!("{field} must be {min}-{max} characters")));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::InvalidProfile(format!("{field} contains control characters")));
    }
    Ok(())
}

/// Like [`check_text`] but also without whitespace.
fn check_token(field: &str, value: &str, min: usize, max: usize) -> Result<()> {
    check_text(field, value, min, max)?;
    if value.chars().any(char::is_whitespace) {
        return Err(Error::InvalidProfile(format!("{field} contains whitespace")));
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<()> {
    check_token("host", host, 1, MAX_HOST_LEN)?;
    if host.starts_with('-') {
        return Err(Error::InvalidProfile("host must not start with '-'".into()));
    }
    let ok = host
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'));
    if !ok {
        return Err(Error::InvalidProfile("host contains invalid characters".into()));
    }
    Ok(())
}

/// Parses a quick-connect string: `[user@]host[:port]` or `[user@][v6addr][:port]`.
pub fn parse_quick_connect(input: &str, default_user: &str) -> Result<SshTarget> {
    let input = input.trim();
    let input = input.strip_prefix("ssh://").unwrap_or(input);
    let (user, rest) = match input.rsplit_once('@') {
        Some((u, r)) => (u.to_owned(), r),
        None => (default_user.to_owned(), input),
    };

    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (host, after) = v6
            .split_once(']')
            .ok_or_else(|| Error::InvalidProfile("unterminated '['".into()))?;
        let port = match after.strip_prefix(':') {
            Some(p) => parse_port(p)?,
            None if after.is_empty() => DEFAULT_SSH_PORT,
            None => return Err(Error::InvalidProfile("unexpected text after ']'".into())),
        };
        (host.to_owned(), port)
    } else if rest.matches(':').count() == 1 {
        let (h, p) = rest.split_once(':').unwrap_or((rest, ""));
        (h.to_owned(), parse_port(p)?)
    } else {
        // Plain host name, or a bare IPv6 address without a port.
        (rest.to_owned(), DEFAULT_SSH_PORT)
    };

    let mut target = SshTarget {
        host,
        port,
        username: user,
        auth: SshAuth::Auto,
        keepalive_secs: default_keepalive(),
    };
    target.validate()?;
    Ok(target)
}

fn parse_port(p: &str) -> Result<u16> {
    match p.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(Error::InvalidProfile(format!("invalid port '{p}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh(host: &str, user: &str) -> Profile {
        Profile::new(
            "srv",
            ProfileKind::Ssh(SshTarget {
                host: host.into(),
                port: 22,
                username: user.into(),
                auth: SshAuth::Auto,
                keepalive_secs: 30,
            }),
        )
    }

    #[test]
    fn quick_connect_variants() {
        let t = parse_quick_connect("root@example.com", "me").unwrap();
        assert_eq!(
            (t.username.as_str(), t.host.as_str(), t.port),
            ("root", "example.com", 22)
        );

        let t = parse_quick_connect("example.com:2222", "me").unwrap();
        assert_eq!((t.username.as_str(), t.port), ("me", 2222));

        let t = parse_quick_connect("ssh://admin@[2001:db8::1]:2200", "me").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("2001:db8::1", 2200));

        let t = parse_quick_connect("fe80::1", "me").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("fe80::1", 22));

        let t = parse_quick_connect("user@domain.local@10.0.0.1", "me").unwrap();
        assert_eq!(
            (t.username.as_str(), t.host.as_str()),
            ("user@domain.local", "10.0.0.1")
        );
    }

    #[test]
    fn quick_connect_rejects_garbage() {
        assert!(parse_quick_connect("host:0", "me").is_err());
        assert!(parse_quick_connect("host:99999", "me").is_err());
        assert!(parse_quick_connect("-oProxyCommand=evil", "me").is_err());
        assert!(parse_quick_connect("bad host", "me").is_err());
        assert!(parse_quick_connect("[::1", "me").is_err());
        assert!(parse_quick_connect("", "me").is_err());
    }

    #[test]
    fn validation() {
        assert!(ssh("example.com", "root").validate().is_ok());
        assert!(ssh("exa mple.com", "root").validate().is_err());
        assert!(ssh("example.com", "ro\not").validate().is_err());
        assert!(ssh("host;rm -rf", "root").validate().is_err());

        let mut p = ssh("h", "u");
        p.color = Some("#12ab9F".into());
        assert!(p.validate().is_ok());
        p.color = Some("red".into());
        assert!(p.validate().is_err());

        let mut p = ssh("h", "u");
        p.name = "  ".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn display() {
        let mut t = parse_quick_connect("u@h", "x").unwrap();
        assert_eq!(t.display(), "u@h");
        t.port = 2222;
        assert_eq!(t.display(), "u@h:2222");
        let t = parse_quick_connect("u@[::1]:2", "x").unwrap();
        assert_eq!(t.display(), "u@[::1]:2");
    }

    #[test]
    fn serde_shape_is_stable() {
        let p = ssh("h", "u");
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["type"], "ssh");
        assert_eq!(json["auth"]["method"], "auto");
        let back: Profile = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }
}
