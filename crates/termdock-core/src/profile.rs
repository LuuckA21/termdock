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
const MAX_STARTUP_LEN: usize = 1024;
const MAX_ARGS: usize = 64;
const MAX_ARG_LEN: usize = 4096;
const MAX_FORWARDS: usize = 32;
/// Longest ProxyJump chain accepted (OpenSSH has no limit; this guards loops).
const MAX_JUMPS: usize = 8;

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
    /// A production server: its tabs and terminals are marked in red.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub production: bool,
    /// One line typed into the shell (with Enter) once the session starts,
    /// e.g. `cd /var/www`; again after a reconnection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<String>,
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
    /// Saved SSH profile to connect through (ProxyJump). That profile may have
    /// its own jump, forming a chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<Uuid>,
    /// Port forwards opened once the session is authenticated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forwards: Vec<Forward>,
}

/// A port forward, as in OpenSSH's `-L`, `-R` and `-D`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Forward {
    /// `-L`: listen on this machine, connections leave from the server.
    #[serde(rename_all = "camelCase")]
    Local {
        #[serde(default = "loopback")]
        bind_host: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
    },
    /// `-R`: listen on the server, connections leave from this machine.
    #[serde(rename_all = "camelCase")]
    Remote {
        #[serde(default = "remote_loopback")]
        bind_host: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
    },
    /// `-D`: local SOCKS5 proxy, connections leave from the server.
    #[serde(rename_all = "camelCase")]
    Dynamic {
        #[serde(default = "loopback")]
        bind_host: String,
        bind_port: u16,
    },
}

fn loopback() -> String {
    "127.0.0.1".into()
}

fn remote_loopback() -> String {
    "localhost".into()
}

impl Forward {
    /// Short human readable form, e.g. `L 127.0.0.1:8080 -> db:5432`.
    pub fn describe(&self) -> String {
        match self {
            Forward::Local {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => format!(
                "L {} -> {}",
                host_port(bind_host, *bind_port),
                host_port(dest_host, *dest_port)
            ),
            Forward::Remote {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => format!(
                "R {} -> {}",
                host_port(bind_host, *bind_port),
                host_port(dest_host, *dest_port)
            ),
            Forward::Dynamic { bind_host, bind_port } => format!("D {} (SOCKS5)", host_port(bind_host, *bind_port)),
        }
    }

    fn validate(&mut self) -> Result<()> {
        let (bind_host, bind_port, dest) = match self {
            Forward::Local {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            }
            | Forward::Remote {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => (bind_host, bind_port, Some((dest_host, dest_port))),
            Forward::Dynamic { bind_host, bind_port } => (bind_host, bind_port, None),
        };
        *bind_host = bind_host.trim().to_owned();
        validate_host(bind_host)?;
        if *bind_port == 0 {
            return Err(Error::InvalidProfile("forward ports must be 1-65535".into()));
        }
        if let Some((host, port)) = dest {
            *host = host.trim().to_owned();
            validate_host(host)?;
            if *port == 0 {
                return Err(Error::InvalidProfile("forward ports must be 1-65535".into()));
            }
        }
        Ok(())
    }

    /// The local socket this forward listens on, if any.
    fn local_bind(&self) -> Option<(&str, u16)> {
        match self {
            Forward::Local {
                bind_host, bind_port, ..
            }
            | Forward::Dynamic { bind_host, bind_port } => Some((bind_host, *bind_port)),
            Forward::Remote { .. } => None,
        }
    }
}

fn host_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
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
            production: false,
            startup: None,
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

        if let Some(startup) = &mut self.startup {
            *startup = startup.trim().to_owned();
            if startup.is_empty() {
                self.startup = None;
            } else {
                check_text("startup command", startup, 1, MAX_STARTUP_LEN)?;
                if startup.chars().any(crate::sftp::is_format_char) {
                    return Err(Error::InvalidProfile(
                        "startup command contains invisible characters".into(),
                    ));
                }
            }
        }

        match &mut self.kind {
            ProfileKind::Ssh(t) => {
                if t.jump == Some(self.id) {
                    return Err(Error::InvalidProfile("a connection cannot jump through itself".into()));
                }
                t.validate()
            }
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
        if self.forwards.len() > MAX_FORWARDS {
            return Err(Error::InvalidProfile(format!("at most {MAX_FORWARDS} port forwards")));
        }
        let mut binds = std::collections::HashSet::new();
        let mut remote_binds = std::collections::HashSet::new();
        for f in &mut self.forwards {
            f.validate()?;
            let (host, port, fresh) = match f {
                Forward::Remote {
                    bind_host, bind_port, ..
                } => (
                    bind_host.as_str(),
                    *bind_port,
                    remote_binds.insert((bind_host.to_lowercase(), *bind_port)),
                ),
                _ => match f.local_bind() {
                    Some((host, port)) => (host, port, binds.insert((host.to_lowercase(), port))),
                    None => continue,
                },
            };
            if !fresh {
                return Err(Error::InvalidProfile(format!("{host}:{port} is used by two forwards")));
            }
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
        // Shown wherever the connection is (manager, terminal): no control
        // or invisible characters that could hide or rewrite what runs.
        let hidden = |c: char| c.is_control() || crate::sftp::is_format_char(c);
        if self.program.chars().any(hidden) {
            return Err(Error::InvalidProfile("program contains invisible characters".into()));
        }
        for arg in &self.args {
            if arg.len() > MAX_ARG_LEN || arg.chars().any(hidden) {
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
        jump: None,
        forwards: Vec::new(),
    };
    target.validate()?;
    Ok(target)
}

/// One hop of a connection: the saved profile it comes from and its target.
#[derive(Debug, Clone)]
pub struct Hop {
    pub profile_id: Option<Uuid>,
    pub target: SshTarget,
}

/// Expands the ProxyJump chain of `target`: returns the hops to go through,
/// outermost (first to connect) first, without `target` itself.
pub fn resolve_jumps(profiles: &[Profile], target: &SshTarget) -> Result<Vec<Hop>> {
    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut next = target.jump;
    while let Some(id) = next {
        if !seen.insert(id) {
            return Err(Error::InvalidProfile("jump hosts form a loop".into()));
        }
        if chain.len() >= MAX_JUMPS {
            return Err(Error::InvalidProfile(format!("more than {MAX_JUMPS} jump hosts")));
        }
        let profile = profiles.iter().find(|p| p.id == id).ok_or(Error::ProfileNotFound(id))?;
        let ProfileKind::Ssh(hop) = &profile.kind else {
            return Err(Error::InvalidProfile(format!(
                "jump host '{}' is not an SSH connection",
                profile.name
            )));
        };
        next = hop.jump;
        chain.push(Hop {
            profile_id: Some(profile.id),
            // Forwards of a bastion profile only apply when it is opened directly.
            target: SshTarget {
                forwards: Vec::new(),
                ..hop.clone()
            },
        });
    }
    chain.reverse();
    Ok(chain)
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
                jump: None,
                forwards: Vec::new(),
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
    fn startup_command_is_one_visible_line() {
        let mut p = ssh("h", "u");
        p.startup = Some("  cd /var/www && ls  ".into());
        p.validate().unwrap();
        assert_eq!(p.startup.as_deref(), Some("cd /var/www && ls"));
        p.startup = Some("   ".into());
        p.validate().unwrap();
        assert_eq!(p.startup, None);
        for bad in [
            "cd /\nrm -rf ~",
            "cd /\rrm -rf ~",
            "echo \u{1b}[2J",
            "ls \u{202e}x",
            &"x".repeat(MAX_STARTUP_LEN + 1),
        ] {
            p.startup = Some(bad.into());
            assert!(p.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn local_shell_arguments_are_visible_text() {
        let local = |program: &str, args: &[&str]| {
            Profile::new(
                "sh",
                ProfileKind::Local(LocalShell {
                    program: program.into(),
                    args: args.iter().map(|a| (*a).to_owned()).collect(),
                    cwd: None,
                }),
            )
        };
        assert!(local("/bin/bash", &["--login", "-c", "echo 'a b'"]).validate().is_ok());
        for (program, arg) in [
            ("/bin/sh", "\u{1b}]0;x\u{7}"),
            ("/bin/sh", "a\nb"),
            ("/bin/sh", "tab\there"),
            ("/bin/sh", "ls \u{202e}x"),
            ("/bin/sh", "nul\0"),
            ("/bin/s\u{200b}h", "-l"),
        ] {
            assert!(local(program, &[arg]).validate().is_err(), "{program:?} {arg:?}");
        }
    }

    #[test]
    fn new_fields_are_optional_in_stored_data() {
        let json =
            r#"{"id":"6f1c2b9e-8a7d-4c3b-9e2f-1a2b3c4d5e6f","name":"srv","type":"ssh","host":"h","username":"u"}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert!(!p.production);
        assert_eq!(p.startup, None);
        // Not written when unset, so older versions read the file unchanged.
        let out = serde_json::to_string(&p).unwrap();
        assert!(!out.contains("production") && !out.contains("startup"), "{out}");
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

    fn ssh_mut(p: &mut Profile) -> &mut SshTarget {
        match &mut p.kind {
            ProfileKind::Ssh(t) => t,
            ProfileKind::Local(_) => unreachable!(),
        }
    }

    #[test]
    fn forwards_validation() {
        let mut p = ssh("h", "u");
        ssh_mut(&mut p).forwards = vec![
            Forward::Local {
                bind_host: " 127.0.0.1 ".into(),
                bind_port: 8080,
                dest_host: "db".into(),
                dest_port: 5432,
            },
            Forward::Dynamic {
                bind_host: "127.0.0.1".into(),
                bind_port: 1080,
            },
            Forward::Remote {
                bind_host: "localhost".into(),
                bind_port: 8080,
                dest_host: "127.0.0.1".into(),
                dest_port: 3000,
            },
        ];
        assert!(p.validate().is_ok());
        assert_eq!(ssh_mut(&mut p).forwards[0].describe(), "L 127.0.0.1:8080 -> db:5432");

        // Two local listeners on the same socket.
        ssh_mut(&mut p).forwards.push(Forward::Dynamic {
            bind_host: "127.0.0.1".into(),
            bind_port: 8080,
        });
        assert!(p.validate().is_err());

        let mut p = ssh("h", "u");
        ssh_mut(&mut p).forwards = vec![Forward::Local {
            bind_host: "127.0.0.1".into(),
            bind_port: 0,
            dest_host: "db".into(),
            dest_port: 1,
        }];
        assert!(p.validate().is_err());
        ssh_mut(&mut p).forwards = vec![Forward::Local {
            bind_host: "127.0.0.1".into(),
            bind_port: 1,
            dest_host: "db;rm".into(),
            dest_port: 1,
        }];
        assert!(p.validate().is_err());
    }

    #[test]
    fn jump_chain() {
        let mut a = ssh("bastion-a", "u");
        let mut b = ssh("bastion-b", "u");
        let mut c = ssh("target", "u");
        ssh_mut(&mut b).jump = Some(a.id);
        ssh_mut(&mut c).jump = Some(b.id);
        ssh_mut(&mut b).forwards.push(Forward::Dynamic {
            bind_host: "127.0.0.1".into(),
            bind_port: 1080,
        });
        let profiles = vec![a.clone(), b.clone(), c.clone()];

        let target = ssh_mut(&mut c).clone();
        let hops = resolve_jumps(&profiles, &target).unwrap();
        let hosts: Vec<_> = hops.iter().map(|h| h.target.host.as_str()).collect();
        assert_eq!(hosts, ["bastion-a", "bastion-b"]);
        assert!(
            hops[1].target.forwards.is_empty(),
            "bastion forwards must not be opened"
        );
        assert_eq!(hops[0].profile_id, Some(a.id));

        // Loop a -> c -> b -> a.
        ssh_mut(&mut a).jump = Some(c.id);
        let profiles = vec![a.clone(), b, c];
        assert!(resolve_jumps(&profiles, &target).is_err());

        // Missing profile.
        assert!(resolve_jumps(&[], &target).is_err());

        // Self jump is refused at save time.
        ssh_mut(&mut a).jump = Some(a.id);
        assert!(a.validate().is_err());
    }

    #[test]
    fn old_profiles_still_load() {
        let json = r#"{"id":"6f1c2a52-6f0e-4a8e-9d55-2d6f8a3c7b10","name":"x","type":"ssh","host":"h","port":22,"username":"u","auth":{"method":"auto"},"keepaliveSecs":30}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        let ProfileKind::Ssh(t) = p.kind else { panic!() };
        assert!(t.jump.is_none() && t.forwards.is_empty());
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
