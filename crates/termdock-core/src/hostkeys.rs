//! Server host-key verification (protection against man-in-the-middle).
//!
//! Keys are checked against Termdock's own `known_hosts` file and, read-only,
//! against the user's OpenSSH `~/.ssh/known_hosts`. Policy:
//!
//! * key recorded and equal            -> trusted, connect silently
//! * no key recorded for this host     -> ask the user, showing the SHA-256
//!   fingerprint (trust on first use); only an explicit "yes" records it
//! * a different key of the same type  -> refuse, no override from the dialog
//! * key marked `@revoked`             -> refuse
//!
//! Files use the OpenSSH format, so entries can be inspected or edited with
//! standard tools.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use data_encoding::BASE64;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;

use crate::error::{Error, Result};
use crate::store::create_private_dir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    Trusted,
    Unknown,
    Changed { file: PathBuf, line: usize },
    Revoked { file: PathBuf, line: usize },
}

/// What the user is shown before trusting an unknown host.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
}

impl HostKeyInfo {
    pub fn new(host: &str, port: u16, key: &PublicKey) -> Self {
        Self {
            host: host.to_owned(),
            port,
            algorithm: key.algorithm().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KnownHosts {
    /// Termdock's own file; the only one we ever write to.
    app_file: PathBuf,
    /// Additional read-only files (the user's OpenSSH known_hosts).
    extra: Vec<PathBuf>,
}

impl KnownHosts {
    pub fn new(app_file: impl Into<PathBuf>) -> Self {
        Self {
            app_file: app_file.into(),
            extra: Vec::new(),
        }
    }

    /// Also consult `~/.ssh/known_hosts` (read-only).
    pub fn with_openssh_user_file(mut self) -> Self {
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_owned()) {
            self.extra.push(home.join(".ssh").join("known_hosts"));
        }
        self
    }

    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus> {
        let mut changed = None;
        let mut trusted = false;
        for file in std::iter::once(&self.app_file).chain(&self.extra) {
            match check_file(file, host, port, key)? {
                HostKeyStatus::Revoked { file, line } => {
                    return Ok(HostKeyStatus::Revoked { file, line });
                }
                HostKeyStatus::Trusted => trusted = true,
                c @ HostKeyStatus::Changed { .. } => {
                    changed.get_or_insert(c);
                }
                HostKeyStatus::Unknown => {}
            }
        }
        Ok(if trusted {
            HostKeyStatus::Trusted
        } else {
            changed.unwrap_or(HostKeyStatus::Unknown)
        })
    }

    /// Records a key the user explicitly accepted.
    pub fn learn(&self, host: &str, port: u16, key: &PublicKey) -> Result<()> {
        if let Some(dir) = self.app_file.parent() {
            create_private_dir(dir)?;
        }
        let line = format!(
            "{} {}\n",
            host_pattern(host, port),
            key.to_openssh().map_err(|e| Error::SshKey(e.into()))?
        );
        let mut opts = fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&self.app_file)?;
        f.write_all(line.as_bytes())?;
        f.sync_all()?;
        Ok(())
    }

    /// Removes every key Termdock recorded for `host:port`, e.g. after a
    /// legitimate server re-install. Returns how many entries were removed.
    pub fn forget(&self, host: &str, port: u16) -> Result<usize> {
        let content = match fs::read_to_string(&self.app_file) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let target = host_pattern(host, port).to_lowercase();
        let mut removed = 0;
        let kept: String = content
            .lines()
            .filter(|line| {
                let matches = parse_line(line).is_some_and(|e| hosts_match(e.hosts, &target));
                removed += usize::from(matches);
                !matches
            })
            .map(|l| format!("{l}\n"))
            .collect();
        if removed > 0 {
            let dir = self.app_file.parent().unwrap_or(Path::new("."));
            let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
            tmp.write_all(kept.as_bytes())?;
            tmp.as_file().sync_all()?;
            tmp.persist(&self.app_file).map_err(|e| Error::Io(e.error))?;
        }
        Ok(removed)
    }
}

fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
}

struct Entry<'a> {
    marker: Option<&'a str>,
    hosts: &'a str,
    key_b64: &'a str,
}

fn parse_line(line: &str) -> Option<Entry<'_>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split_ascii_whitespace();
    let mut first = fields.next()?;
    let marker = if first.starts_with('@') {
        let m = first;
        first = fields.next()?;
        Some(m)
    } else {
        None
    };
    let _key_type = fields.next()?;
    let key_b64 = fields.next()?;
    Some(Entry {
        marker,
        hosts: first,
        key_b64,
    })
}

fn check_file(file: &Path, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus> {
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HostKeyStatus::Unknown),
        Err(e) => return Err(e.into()),
    };
    let target = host_pattern(host, port).to_lowercase();

    let mut status = HostKeyStatus::Unknown;
    for (idx, line) in content.lines().enumerate() {
        let Some(entry) = parse_line(line) else { continue };
        // Unparseable keys are skipped rather than failing the whole check,
        // like OpenSSH does.
        let Ok(recorded) = russh::keys::parse_public_key_base64(entry.key_b64) else {
            continue;
        };
        let same_key = recorded.key_data() == key.key_data();

        if entry.marker == Some("@revoked") {
            if same_key {
                return Ok(HostKeyStatus::Revoked {
                    file: file.to_owned(),
                    line: idx + 1,
                });
            }
            continue;
        }
        if entry.marker.is_some() || !hosts_match(entry.hosts, &target) {
            // @cert-authority (we do not accept host certificates) or another host.
            continue;
        }
        if same_key {
            status = HostKeyStatus::Trusted;
        } else if recorded.algorithm() == key.algorithm() && status == HostKeyStatus::Unknown {
            status = HostKeyStatus::Changed {
                file: file.to_owned(),
                line: idx + 1,
            };
        }
    }
    Ok(status)
}

/// OpenSSH host list matching: comma separated patterns with `*`/`?`
/// wildcards, `!` negation and `|1|salt|hash` hashed names.
fn hosts_match(patterns: &str, target: &str) -> bool {
    let mut matched = false;
    for pat in patterns.split(',') {
        let (negated, pat) = match pat.strip_prefix('!') {
            Some(p) => (true, p),
            None => (false, pat),
        };
        let hit = if let Some(hashed) = pat.strip_prefix("|1|") {
            hashed_match(hashed, target)
        } else {
            wildcard_match(&pat.to_lowercase(), target)
        };
        if hit {
            if negated {
                return false;
            }
            matched = true;
        }
    }
    matched
}

fn hashed_match(hashed: &str, target: &str) -> bool {
    let Some((salt, hash)) = hashed.split_once('|') else {
        return false;
    };
    let (Ok(salt), Ok(hash)) = (BASE64.decode(salt.as_bytes()), BASE64.decode(hash.as_bytes())) else {
        return false;
    };
    let Ok(mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.chain_update(target.as_bytes()).verify_slice(&hash).is_ok()
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED_A: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const ED_B: &str = "AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";

    fn key(b64: &str) -> PublicKey {
        russh::keys::parse_public_key_base64(b64).unwrap()
    }

    #[test]
    fn tofu_flow() {
        let dir = tempfile::tempdir().unwrap();
        let kh = KnownHosts::new(dir.path().join("sub").join("known_hosts"));
        let a = key(ED_A);
        let b = key(ED_B);

        assert_eq!(kh.check("example.com", 22, &a).unwrap(), HostKeyStatus::Unknown);
        kh.learn("example.com", 22, &a).unwrap();
        kh.learn("example.com", 2222, &b).unwrap();
        assert_eq!(kh.check("example.com", 22, &a).unwrap(), HostKeyStatus::Trusted);
        assert_eq!(kh.check("EXAMPLE.com", 22, &a).unwrap(), HostKeyStatus::Trusted);
        assert_eq!(kh.check("example.com", 2222, &b).unwrap(), HostKeyStatus::Trusted);
        assert!(matches!(
            kh.check("example.com", 22, &b).unwrap(),
            HostKeyStatus::Changed { line: 1, .. }
        ));
        assert_eq!(kh.check("other", 22, &a).unwrap(), HostKeyStatus::Unknown);

        assert_eq!(kh.forget("example.com", 22).unwrap(), 1);
        assert_eq!(kh.check("example.com", 22, &b).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(kh.check("example.com", 2222, &b).unwrap(), HostKeyStatus::Trusted);
    }

    #[test]
    fn openssh_file_features() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user_known_hosts");
        fs::write(
            &user,
            format!(
                "# comment\n\
                 @cert-authority *.corp ssh-ed25519 {ED_B}\n\
                 bogus line\n\
                 *.example.org,!evil.example.org\tssh-ed25519   {ED_A} comment here\n\
                 |1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= ssh-ed25519 {ED_B}\n\
                 @revoked * ssh-ed25519 {ED_B}\n"
            ),
        )
        .unwrap();
        let mut kh = KnownHosts::new(dir.path().join("app"));
        kh.extra.push(user);

        let a = key(ED_A);
        let b = key(ED_B);
        assert_eq!(kh.check("www.example.org", 22, &a).unwrap(), HostKeyStatus::Trusted);
        assert_eq!(kh.check("evil.example.org", 22, &a).unwrap(), HostKeyStatus::Unknown);
        assert!(matches!(
            kh.check("anything", 22, &b).unwrap(),
            HostKeyStatus::Revoked { line: 6, .. }
        ));
    }

    #[test]
    fn hashed_hosts() {
        // Entry taken from the russh test-suite: hashes "example.com".
        assert!(hosts_match(
            "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak=",
            "example.com"
        ));
        assert!(!hosts_match(
            "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak=",
            "example.org"
        ));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.example.com", "a.example.com"));
        assert!(!wildcard_match("*.example.com", "example.com"));
        assert!(wildcard_match("192.168.1.?", "192.168.1.7"));
        assert!(wildcard_match("[host]:2222", "[host]:2222"));
        assert!(!wildcard_match("[host]:2222", "[host]:22"));
    }
}
