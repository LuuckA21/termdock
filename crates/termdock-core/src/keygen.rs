//! SSH key pairs for the user: a new Ed25519 key, saved in OpenSSH format
//! (private key encrypted with a passphrase unless the user chose none),
//! and public keys read back to be installed on a server.

use std::io::Write;
use std::path::Path;

use russh::keys::ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

/// Longest key comment accepted.
pub const MAX_COMMENT: usize = 100;
/// A public key file larger than this is not a public key.
const MAX_PUBLIC_FILE: usize = 16 * 1024;

/// A generated key pair, ready to be written.
pub struct KeyPair {
    /// The private key in OpenSSH format (encrypted when a passphrase was given).
    pub private: Zeroizing<String>,
    pub public: PublicKeyLine,
}

/// One public key, in the form `authorized_keys` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeyLine {
    /// `algorithm base64 [comment]`, re-encoded from the parsed key.
    pub line: String,
    /// `SHA256:…`, as `ssh-keygen -l` shows it.
    pub fingerprint: String,
    pub comment: String,
    /// The key itself (`algorithm base64`), to find it in a file.
    blob: String,
}

impl PublicKeyLine {
    fn new(key: &PublicKey) -> Result<Self> {
        let line = key.to_openssh().map_err(|e| Error::SshKey(e.into()))?;
        let blob = line.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        Ok(Self {
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            comment: key.comment().to_string(),
            line,
            blob,
        })
    }

    /// Whether `authorized_keys` content already lists this key (with or
    /// without options and comment).
    pub fn listed_in(&self, authorized_keys: &str) -> bool {
        let mut parts = self.blob.split(' ');
        let (Some(algorithm), Some(data)) = (parts.next(), parts.next()) else {
            return false;
        };
        authorized_keys.lines().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.windows(2).any(|w| w[0] == algorithm && w[1] == data)
        })
    }
}

/// A comment as `ssh-keygen -C` takes it: printable, one line, at most
/// [`MAX_COMMENT`] characters.
pub fn validate_comment(comment: &str) -> Result<()> {
    if comment.chars().count() > MAX_COMMENT {
        return Err(Error::InvalidName(format!(
            "comment longer than {MAX_COMMENT} characters"
        )));
    }
    if comment
        .chars()
        .any(|c| c.is_control() || crate::sftp::is_format_char(c))
    {
        return Err(Error::InvalidName("control characters in the comment".into()));
    }
    Ok(())
}

/// A new Ed25519 key from the system's random generator. With a passphrase
/// the private key is encrypted (bcrypt-pbkdf, AES-256), like `ssh-keygen`.
pub fn generate(comment: &str, passphrase: Option<&str>) -> Result<KeyPair> {
    validate_comment(comment)?;
    let mut rng = rand_core::UnwrapErr(rand::rngs::SysRng);
    let mut key = PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(|e| Error::SshKey(e.into()))?;
    key.set_comment(comment);
    let public = PublicKeyLine::new(key.public_key())?;
    if let Some(passphrase) = passphrase.filter(|p| !p.is_empty()) {
        key = key
            .encrypt(&mut rand::rngs::SysRng, passphrase)
            .map_err(|e| Error::SshKey(e.into()))?;
    }
    let private = key.to_openssh(LineEnding::LF).map_err(|e| Error::SshKey(e.into()))?;
    Ok(KeyPair { private, public })
}

/// Writes `pair` as `path` (private, `0600` on Unix) and `path.pub`. Never
/// replaces a file: if either exists nothing is written.
pub fn save(pair: &KeyPair, path: &Path) -> Result<()> {
    let public_path = public_path(path);
    let mut private = new_file(path, 0o600)?;
    let written = (|| {
        private.write_all(pair.private.as_bytes())?;
        private.sync_all()?;
        let mut public = new_file(&public_path, 0o644)?;
        public.write_all(format!("{}\n", pair.public.line).as_bytes())?;
        public.sync_all()
    })();
    if let Err(e) = written {
        // Only what this call created is removed.
        drop(private);
        let _ = std::fs::remove_file(path);
        return Err(e.into());
    }
    Ok(())
}

/// `path` with `.pub` added (`id_ed25519` → `id_ed25519.pub`).
pub fn public_path(path: &Path) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".pub");
    name.into()
}

fn new_file(path: &Path, mode: u32) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

/// The public key in `text` (the content of a `.pub` file): exactly one key,
/// no options. Re-encoded, so nothing but the key and its comment is kept.
pub fn parse_public(text: &str) -> Result<PublicKeyLine> {
    let invalid = |why: &str| Error::InvalidName(format!("not a public key: {why}"));
    if text.len() > MAX_PUBLIC_FILE {
        return Err(invalid("the file is too large"));
    }
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let line = lines.next().ok_or_else(|| invalid("the file is empty"))?;
    if lines.next().is_some() {
        return Err(invalid("the file holds more than one key"));
    }
    if line.contains("PRIVATE KEY") {
        return Err(invalid("this is a private key: choose the .pub file"));
    }
    let key = PublicKey::from_openssh(line).map_err(|e| invalid(&e.to_string()))?;
    validate_comment(key.comment().as_str_lossy()).map_err(|_| invalid("its comment has control characters"))?;
    PublicKeyLine::new(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_round_trip() {
        let pair = generate("me@laptop", Some("correct horse")).unwrap();
        assert!(pair.private.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        let private = PrivateKey::from_openssh(pair.private.as_bytes()).unwrap();
        assert!(private.is_encrypted());
        let decrypted = private.decrypt("correct horse").unwrap();
        assert_eq!(PublicKeyLine::new(decrypted.public_key()).unwrap(), pair.public);
        assert!(pair.public.line.starts_with("ssh-ed25519 "));
        assert!(pair.public.line.ends_with(" me@laptop"));
        assert!(pair.public.fingerprint.starts_with("SHA256:"));

        let plain = generate("", None).unwrap();
        assert!(
            !PrivateKey::from_openssh(plain.private.as_bytes())
                .unwrap()
                .is_encrypted()
        );
        assert_ne!(plain.public.line, pair.public.line);
    }

    #[test]
    fn comments_are_one_printable_line() {
        assert!(validate_comment("Luca's laptop 2026").is_ok());
        for bad in [
            "a\nb",
            "a\rb",
            "tab\there",
            "bidi\u{202e}x",
            &"x".repeat(MAX_COMMENT + 1),
        ] {
            assert!(validate_comment(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn saving_never_replaces_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519");
        let pair = generate("x", None).unwrap();
        save(&pair, &path).unwrap();
        let written = std::fs::read_to_string(public_path(&path)).unwrap();
        assert_eq!(parse_public(&written).unwrap(), pair.public);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // Either file existing stops it, and leaves nothing new behind.
        let other = generate("y", None).unwrap();
        assert!(save(&other, &path).is_err());
        assert_eq!(std::fs::read_to_string(public_path(&path)).unwrap(), written);
        let fresh = dir.path().join("fresh");
        std::fs::write(public_path(&fresh), "keep").unwrap();
        assert!(save(&other, &fresh).is_err());
        assert!(!fresh.exists());
        assert_eq!(std::fs::read_to_string(public_path(&fresh)).unwrap(), "keep");
    }

    #[test]
    fn public_keys_are_parsed_strictly() {
        let pair = generate("c", None).unwrap();
        let line = &pair.public.line;
        assert_eq!(parse_public(&format!("# mine\n\n{line}\n")).unwrap().line, *line);
        for bad in [
            String::new(),
            format!("{line}\n{line}"),
            format!("command=\"rm -rf /\" {line}"),
            "-----BEGIN OPENSSH PRIVATE KEY-----".into(),
            "ssh-ed25519 not-base64".into(),
            format!("{line}\u{1b}[31m"),
        ] {
            assert!(parse_public(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn finds_keys_already_authorized() {
        let pair = generate("new", None).unwrap();
        let blob: Vec<&str> = pair.public.line.split(' ').take(2).collect();
        let other = generate("other", None).unwrap();
        assert!(!pair.public.listed_in(&format!("{}\n", other.public.line)));
        assert!(
            pair.public
                .listed_in(&format!("{}\n{} old comment\n", other.public.line, blob.join(" ")))
        );
        assert!(
            pair.public
                .listed_in(&format!("from=\"10.0.0.0/8\",no-pty {}", blob.join(" ")))
        );
        assert!(!pair.public.listed_in(""));
    }
}
