//! End-to-end tests against a real OpenSSH server.
//!
//! Skipped unless `TERMDOCK_E2E_SSH` is set; see `scripts/e2e-sshd.sh`, which
//! starts a throw-away sshd and exports the variables used here:
//!
//! * `TERMDOCK_E2E_SSH`      `host:port`
//! * `TERMDOCK_E2E_USER`     login user
//! * `TERMDOCK_E2E_PASSWORD` its password
//! * `TERMDOCK_E2E_KEY`      private key (passphrase `TERMDOCK_E2E_KEY_PASS`)
//!
//! The server must offer the `sftp` subsystem (the script enables it).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use termdock_core::Error;
use termdock_core::hostkeys::{HostKeyInfo, KnownHosts};
use termdock_core::profile::{Forward, Hop, SshAuth, SshTarget};
use termdock_core::session::{
    BoxFuture, EventSink, Interaction, SecretAnswer, SecretPrompt, SessionEvent, SessionHandle, TermSize,
};
#[cfg(unix)]
use termdock_core::sftp::{self, CancellationToken, EntryKind, OpenAs, Sftp};
use termdock_core::ssh::{self, SshConnect};
use termdock_core::vault::{MemoryVault, SecretKind, Vault};
use uuid::Uuid;
use zeroize::Zeroizing;

struct Env {
    host: String,
    port: u16,
    user: String,
    password: String,
    key: PathBuf,
    key_pass: String,
}

fn env() -> Option<Env> {
    let addr = std::env::var("TERMDOCK_E2E_SSH").ok()?;
    let (host, port) = addr.rsplit_once(':')?;
    Some(Env {
        host: host.into(),
        port: port.parse().ok()?,
        user: std::env::var("TERMDOCK_E2E_USER").ok()?,
        password: std::env::var("TERMDOCK_E2E_PASSWORD").ok()?,
        key: std::env::var("TERMDOCK_E2E_KEY").ok()?.into(),
        key_pass: std::env::var("TERMDOCK_E2E_KEY_PASS").ok()?,
    })
}

/// Scripted user: answers prompts from a queue and records what was asked.
#[derive(Default)]
struct ScriptedUser {
    trust_host: bool,
    answers: Mutex<Vec<(String, bool)>>,
    asked: Mutex<Vec<String>>,
    host_prompts: Mutex<Vec<HostKeyInfo>>,
    infos: Mutex<Vec<String>>,
}

impl Interaction for ScriptedUser {
    fn confirm_host_key(&self, info: HostKeyInfo) -> BoxFuture<'_, bool> {
        self.host_prompts.lock().unwrap().push(info);
        Box::pin(async move { self.trust_host })
    }

    fn ask_secret(&self, prompt: SecretPrompt) -> BoxFuture<'_, Option<SecretAnswer>> {
        let kind = match prompt {
            SecretPrompt::Password { .. } => "password",
            SecretPrompt::KeyPassphrase { .. } => "passphrase",
            SecretPrompt::Challenge { .. } => "challenge",
        };
        self.asked.lock().unwrap().push(kind.into());
        let next = {
            let mut a = self.answers.lock().unwrap();
            if a.is_empty() { None } else { Some(a.remove(0)) }
        };
        Box::pin(async move {
            next.map(|(s, remember)| SecretAnswer {
                secret: Zeroizing::new(s),
                remember,
            })
        })
    }

    fn info(&self, text: String) {
        self.infos.lock().unwrap().push(text);
    }
}

fn user(trust: bool, answers: &[(&str, bool)]) -> Arc<ScriptedUser> {
    Arc::new(ScriptedUser {
        trust_host: trust,
        answers: Mutex::new(answers.iter().map(|(s, r)| ((*s).to_owned(), *r)).collect()),
        ..Default::default()
    })
}

struct Output {
    sink: EventSink,
    text: Arc<Mutex<String>>,
    exit: tokio::sync::oneshot::Receiver<Option<u32>>,
}

fn output() -> Output {
    let text = Arc::new(Mutex::new(String::new()));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let t = text.clone();
    let sink: EventSink = Arc::new(move |ev| match ev {
        SessionEvent::Data(d) => t.lock().unwrap().push_str(&String::from_utf8_lossy(&d)),
        SessionEvent::Exit { code, .. } => {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(code);
            }
        }
    });
    Output { sink, text, exit: rx }
}

async fn connect(
    e: &Env,
    auth: SshAuth,
    kh: &Arc<KnownHosts>,
    vault: &Arc<MemoryVault>,
    profile: Option<Uuid>,
    ui: Arc<ScriptedUser>,
    sink: EventSink,
) -> termdock_core::Result<SessionHandle> {
    ssh::connect(
        SshConnect {
            profile_id: profile,
            target: SshTarget {
                host: e.host.clone(),
                port: e.port,
                username: e.user.clone(),
                auth,
                keepalive_secs: 5,
                jump: None,
                forwards: Vec::new(),
            },
            jumps: Vec::new(),
            size: TermSize { cols: 100, rows: 30 },
            known_hosts: kh.clone(),
            vault: vault.clone(),
            ui,
        },
        sink,
    )
    .await
}

async fn run_and_exit(h: SessionHandle, out: Output, marker: &str) -> String {
    h.write(format!("echo {marker}-$((6*7)); stty size; exit 5\n").into_bytes())
        .unwrap();
    let code = tokio::time::timeout(Duration::from_secs(15), out.exit)
        .await
        .expect("session did not end")
        .unwrap();
    assert_eq!(code, Some(5));
    let text = out.text.lock().unwrap().clone();
    assert!(text.contains(&format!("{marker}-42")), "{text}");
    assert!(text.contains("30 100"), "{text}");
    text
}

/// A host recorded with a key of another type (here a random ECDSA key) is
/// refused like a changed key: never turned into the "unknown host" question.
#[tokio::test(flavor = "multi_thread")]
async fn other_key_type_is_refused() {
    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("known_hosts");
    let pattern = if e.port == 22 {
        e.host.clone()
    } else {
        format!("[{}]:{}", e.host, e.port)
    };
    std::fs::write(
        &file,
        format!(
            "{pattern} ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBJVL/uD4vVTOphUTx2BAEd0UcaR0Yw+Tbd9Tzmq/5WpVf8hrPHbalCA6bUWtBr66/6Aavwh4+BR02Qarw0D5gpw=\n"
        ),
    )
    .unwrap();
    let kh = Arc::new(KnownHosts::new(file));
    let vault = Arc::new(MemoryVault::default());
    let ui = user(true, &[(&e.password, false)]);
    let err = connect(&e, SshAuth::Password, &kh, &vault, None, ui.clone(), output().sink)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::HostKeyChanged { .. }), "{err}");
    assert!(
        ui.host_prompts.lock().unwrap().is_empty(),
        "no trust-on-first-use question"
    );
    assert!(ui.asked.lock().unwrap().is_empty(), "no password asked");
}

#[tokio::test(flavor = "multi_thread")]
async fn ssh_end_to_end() {
    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let profile = Uuid::new_v4();

    // 1. Unknown host, user refuses -> no connection, nothing recorded.
    let ui = user(false, &[]);
    let err = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        output().sink,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::HostKeyRejected(_)), "{err}");
    assert_eq!(ui.host_prompts.lock().unwrap().len(), 1);
    assert!(
        ui.asked.lock().unwrap().is_empty(),
        "no secret may be asked before the host is trusted"
    );
    assert!(!dir.path().join("known_hosts").exists());

    // 2. Trust on first use + password (remembered) + interactive shell.
    let ui = user(true, &[("wrong", false), (&e.password, true)]);
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        out.sink.clone(),
    )
    .await
    .unwrap();
    let fp = ui.host_prompts.lock().unwrap()[0].fingerprint.clone();
    assert!(fp.starts_with("SHA256:"), "{fp}");
    assert_eq!(*ui.asked.lock().unwrap(), ["password", "password"]);
    run_and_exit(h, out, "pw").await;
    assert_eq!(
        vault.get(profile, SecretKind::Password).unwrap().unwrap().as_str(),
        e.password
    );

    // 3. Known host + remembered password: no prompts at all.
    let ui = user(false, &[]);
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        out.sink.clone(),
    )
    .await
    .unwrap();
    assert!(ui.host_prompts.lock().unwrap().is_empty());
    assert!(ui.asked.lock().unwrap().is_empty());
    run_and_exit(h, out, "saved").await;

    // 4. Encrypted private key: passphrase asked, wrong one retried.
    let ui = user(false, &[("nope", false), (&e.key_pass, false)]);
    let out = output();
    let h = connect(
        &e,
        SshAuth::Key { path: e.key.clone() },
        &kh,
        &vault,
        None,
        ui.clone(),
        out.sink.clone(),
    )
    .await
    .unwrap();
    assert_eq!(*ui.asked.lock().unwrap(), ["passphrase", "passphrase"]);
    run_and_exit(h, out, "key").await;

    // 5. Closing from the client side ends the session.
    let ui = user(false, &[]);
    let out = output();
    let h = connect(&e, SshAuth::Password, &kh, &vault, Some(profile), ui, out.sink.clone())
        .await
        .unwrap();
    h.close();
    tokio::time::timeout(Duration::from_secs(10), out.exit)
        .await
        .expect("close did not end the session")
        .unwrap();

    // 6. Wrong password and cancel -> clear errors.
    let ui = user(false, &[("bad1", false), ("bad2", false), ("bad3", false)]);
    let err = connect(&e, SshAuth::Password, &kh, &vault, None, ui, output().sink)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AuthFailed(_)), "{err}");
    let ui = user(false, &[]);
    let err = connect(&e, SshAuth::Password, &kh, &vault, None, ui, output().sink)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AuthCancelled), "{err}");

    // 7. Server key replaced (MITM simulation): refused before authentication.
    let other = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    let pattern = if e.port == 22 {
        e.host.clone()
    } else {
        format!("[{}]:{}", e.host, e.port)
    };
    std::fs::write(
        dir.path().join("known_hosts"),
        format!("{pattern} ssh-ed25519 {other}\n"),
    )
    .unwrap();
    let ui = user(true, &[(&e.password, false)]);
    let err = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        output().sink,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::HostKeyChanged { .. }), "{err}");
    assert!(
        ui.host_prompts.lock().unwrap().is_empty(),
        "a changed key must not be offered for trust"
    );
    assert!(ui.asked.lock().unwrap().is_empty());
}

/// Server with `PasswordAuthentication no` + keyboard-interactive (PAM), as
/// shipped by several distributions. `TERMDOCK_E2E_SSH_KBD` = `host:port`.
#[tokio::test(flavor = "multi_thread")]
async fn ssh_keyboard_interactive() {
    let (Some(e), Ok(addr)) = (env(), std::env::var("TERMDOCK_E2E_SSH_KBD")) else {
        eprintln!("TERMDOCK_E2E_SSH_KBD not set, skipping");
        return;
    };
    let (host, port) = addr.rsplit_once(':').unwrap();
    let e = Env {
        host: host.into(),
        port: port.parse().unwrap(),
        ..e
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let profile = Uuid::new_v4();

    // No agent, no default keys: Auto falls through to keyboard-interactive.
    let ui = user(true, &[(&e.password, true)]);
    let out = output();
    let h = connect(
        &e,
        SshAuth::Auto,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        out.sink.clone(),
    )
    .await
    .unwrap();
    assert_eq!(*ui.asked.lock().unwrap(), ["challenge"]);
    run_and_exit(h, out, "kbd").await;
    assert!(vault.get(profile, SecretKind::Password).unwrap().is_some());

    // The remembered password answers the challenge on the next login.
    let ui = user(false, &[]);
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        out.sink.clone(),
    )
    .await
    .unwrap();
    assert!(ui.asked.lock().unwrap().is_empty());
    run_and_exit(h, out, "kbd2").await;
}

async fn echo_server() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn assert_echo(stream: &mut tokio::net::TcpStream, msg: &[u8]) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(msg).await.unwrap();
    let mut buf = vec![0u8; msg.len()];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut buf))
        .await
        .expect("no echo")
        .unwrap();
    assert_eq!(buf, msg);
}

/// Jump host + `-L`, `-D` and `-R` forwards. The bastion and the target are
/// the same sshd, reached under two names so that each hop is verified on its own.
#[tokio::test(flavor = "multi_thread")]
async fn ssh_jump_and_forwards() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let echo = echo_server().await;
    let (l_port, d_port, r_port) = (free_port(), free_port(), free_port());

    let bastion = SshTarget {
        host: e.host.clone(),
        port: e.port,
        username: e.user.clone(),
        auth: SshAuth::Password,
        keepalive_secs: 5,
        jump: None,
        forwards: Vec::new(),
    };
    let target = SshTarget {
        host: "localhost".into(),
        forwards: vec![
            Forward::Local {
                bind_host: "127.0.0.1".into(),
                bind_port: l_port,
                dest_host: "127.0.0.1".into(),
                dest_port: echo,
            },
            Forward::Dynamic {
                bind_host: "127.0.0.1".into(),
                bind_port: d_port,
            },
            Forward::Remote {
                bind_host: "127.0.0.1".into(),
                bind_port: r_port,
                dest_host: "127.0.0.1".into(),
                dest_port: echo,
            },
        ],
        ..bastion.clone()
    };

    let ui = user(true, &[(&e.password, false), (&e.password, false)]);
    let out = output();
    let h = ssh::connect(
        SshConnect {
            profile_id: None,
            target,
            jumps: vec![Hop {
                profile_id: Some(Uuid::new_v4()),
                target: bastion,
            }],
            size: TermSize { cols: 100, rows: 30 },
            known_hosts: kh.clone(),
            vault: vault.clone(),
            ui: ui.clone(),
        },
        out.sink.clone(),
    )
    .await
    .unwrap();

    // Each hop verified its own host key and authenticated on its own.
    let hosts: Vec<_> = ui.host_prompts.lock().unwrap().iter().map(|i| i.host.clone()).collect();
    assert_eq!(hosts, [e.host.as_str(), "localhost"]);
    assert_eq!(*ui.asked.lock().unwrap(), ["password", "password"]);
    let infos = ui.infos.lock().unwrap().clone();
    assert!(infos[0].starts_with("jump: "), "{infos:?}");
    assert_eq!(infos.iter().filter(|i| i.ends_with(": active")).count(), 3, "{infos:?}");

    // -L
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", l_port)).await.unwrap();
    assert_echo(&mut s, b"through -L\n").await;

    // -D (SOCKS5 CONNECT to the echo server by name)
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", d_port)).await.unwrap();
    s.write_all(&[5, 1, 0]).await.unwrap();
    let mut auth = [0u8; 2];
    s.read_exact(&mut auth).await.unwrap();
    assert_eq!(auth, [5, 0]);
    let mut req = vec![5, 1, 0, 3, 9];
    req.extend(b"localhost");
    req.extend(echo.to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut reply = [0u8; 10];
    s.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0, "SOCKS reply {reply:?}");
    assert_echo(&mut s, b"through -D\n").await;

    // -R: the server listens, we connect to the local echo server.
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", r_port)).await.unwrap();
    assert_echo(&mut s, b"through -R\n").await;

    // The shell works too, and closing it tears the forwards down.
    run_and_exit(h, out, "jump").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", l_port)).await.is_err());
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", d_port)).await.is_err());
}

/// A remembered password that no longer works is used once, then forgotten
/// (replaying it on every login could lock the account) and the user is asked.
#[tokio::test(flavor = "multi_thread")]
async fn stale_saved_password_is_forgotten() {
    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let profile = Uuid::new_v4();
    vault.set(profile, SecretKind::Password, "old-password").unwrap();

    let ui = user(true, &[(&e.password, false)]);
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        Some(profile),
        ui.clone(),
        out.sink.clone(),
    )
    .await
    .unwrap();
    assert_eq!(*ui.asked.lock().unwrap(), ["password"]);
    assert!(vault.get(profile, SecretKind::Password).unwrap().is_none());
    assert!(ui.infos.lock().unwrap().iter().any(|i| i.contains("rejected")));
    run_and_exit(h, out, "stale").await;
}

/// SFTP on the terminal's own connection: browse, create, upload (with and
/// without overwrite, keeping permissions), download, cancel, rename,
/// remove; names that would leave the directory, non-regular files and
/// oversized listings are handled safely.
/// Unix only: it checks Unix permissions and prepares files from the shell.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn sftp_on_terminal_connection() {
    use std::os::unix::fs::PermissionsExt;

    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        None,
        user(true, &[(&e.password, false)]),
        out.sink.clone(),
    )
    .await
    .unwrap();

    let fs = Sftp::open(&h).await.unwrap();
    let home = fs.canonicalize(".").await.unwrap();
    assert!(home.starts_with('/'), "{home}");
    let work = format!("td-sftp-{}", Uuid::new_v4().simple());
    fs.mkdir(&home, &work).await.unwrap();
    let listing = fs.list(&home).await.unwrap();
    assert!(
        listing
            .entries
            .iter()
            .any(|x| x.name == work && x.kind == EntryKind::Dir)
    );
    let remote_dir = sftp::join(&home, &work);
    let no_cancel = CancellationToken::new();

    // Upload 1 MiB + a bit, so several chunks are involved; a new file gets
    // the local permissions.
    let data: Vec<u8> = (0..(1024 * 1024 + 4321)).map(|i| (i * 7 % 251) as u8).collect();
    let local = dir.path().join("src.bin");
    std::fs::write(&local, &data).unwrap();
    std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o640)).unwrap();
    let open = || std::fs::File::open(&local).unwrap();
    let mut seen = 0;
    let n = fs
        .upload(
            open(),
            Some(0o640),
            &remote_dir,
            "data.bin",
            false,
            &no_cancel,
            |done, _| seen = done,
        )
        .await
        .unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(seen, data.len() as u64);
    let mode_of = |l: &sftp::Listing, name: &str| l.entries.iter().find(|x| x.name == name).and_then(|x| x.mode);
    assert_eq!(mode_of(&fs.list(&remote_dir).await.unwrap(), "data.bin"), Some(0o640));

    assert!(matches!(
        fs.upload(
            open(),
            Some(0o644),
            &remote_dir,
            "data.bin",
            false,
            &no_cancel,
            |_, _| {}
        )
        .await,
        Err(Error::Exists(_))
    ));
    // Replacing keeps the remote file's permissions, not the local ones.
    fs.upload(
        open(),
        Some(0o644),
        &remote_dir,
        "data.bin",
        true,
        &no_cancel,
        |_, _| {},
    )
    .await
    .unwrap();
    let listing = fs.list(&remote_dir).await.unwrap();
    assert_eq!(mode_of(&listing, "data.bin"), Some(0o640));
    let names: Vec<_> = listing.entries.into_iter().map(|x| (x.name, x.size)).collect();
    assert_eq!(
        names,
        [("data.bin".to_owned(), Some(data.len() as u64))],
        "no temporary files left"
    );

    // A cancelled upload leaves the original and no temporary file.
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        fs.upload(open(), None, &remote_dir, "data.bin", true, &cancelled, |_, _| {})
            .await,
        Err(Error::Cancelled)
    ));
    assert_eq!(fs.list(&remote_dir).await.unwrap().entries.len(), 1);

    // Download and compare.
    let copy = dir.path().join("copy.bin");
    fs.download(&remote_dir, "data.bin", &copy, &no_cancel, |_, _| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), data);

    // A cancelled download leaves nothing behind.
    let partial = dir.path().join("partial.bin");
    assert!(matches!(
        fs.download(&remote_dir, "data.bin", &partial, &cancelled, |_, _| {})
            .await,
        Err(Error::Cancelled)
    ));
    assert!(!partial.exists());
    assert!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .all(|f| !f.unwrap().file_name().to_string_lossy().ends_with(".part"))
    );

    // Names that would escape the directory are refused before reaching the server.
    for bad in ["../evil", "a/b", "..", "", "x\u{202e}y"] {
        assert!(
            matches!(fs.mkdir(&remote_dir, bad).await, Err(Error::InvalidName(_))),
            "{bad}"
        );
        assert!(matches!(
            fs.rename(&remote_dir, "data.bin", bad).await,
            Err(Error::InvalidName(_))
        ));
    }

    // Non-regular files (here a link to /dev/zero) are not downloaded, and a
    // huge folder is listed only up to the limit. Both prepared from the shell.
    h.write(
        format!(
            "cd {remote_dir} && ln -s /dev/zero zero && mkdir many && (cd many && seq 1 {} | xargs touch) && echo prepared-$((1+1))\n",
            sftp::MAX_ENTRIES + 50
        )
        .into_bytes(),
    )
    .unwrap();
    let ready = async {
        while !out.text.lock().unwrap().contains("prepared-2") {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(60), ready)
        .await
        .expect("shell setup");
    assert!(matches!(
        fs.download(&remote_dir, "zero", &dir.path().join("zero"), &no_cancel, |_, _| {})
            .await,
        Err(Error::NotRegular(_))
    ));
    let many = fs.list(&sftp::join(&remote_dir, "many")).await.unwrap();
    assert!(many.truncated);
    assert_eq!(many.entries.len(), sftp::MAX_ENTRIES);

    fs.rename(&remote_dir, "data.bin", "renamed.bin").await.unwrap();
    // Directories are only removed when empty.
    assert!(fs.remove(&home, &work).await.is_err());
    h.write(format!("rm -rf {remote_dir}/many\n").into_bytes()).unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    fs.remove(&remote_dir, "renamed.bin").await.unwrap();
    fs.remove(&remote_dir, "zero").await.unwrap();
    fs.remove(&home, &work).await.unwrap();
    assert!(!fs.list(&home).await.unwrap().entries.iter().any(|x| x.name == work));

    // The terminal still works on the same connection.
    run_and_exit(h, out, "sftp").await;
}

/// SFTP as another user through sudo, as after `sudo -iu user` in the
/// terminal: without a password (NOPASSWD), with a password that is only sent
/// once sudo prompts (wrong, then right, into the same sudo), sudo giving up
/// after its three tries, refused when sudo does not allow it, and bad user
/// names never reach the server. Needs the sudo rules from e2e-sshd.sh.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn sftp_as_another_user() {
    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let (Ok(other), Ok(nopw)) = (
        std::env::var("TERMDOCK_E2E_SUDO_USER"),
        std::env::var("TERMDOCK_E2E_SUDO_NOPW_USER"),
    ) else {
        eprintln!("sudo test users not set up, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        None,
        user(true, &[(&e.password, false)]),
        out.sink.clone(),
    )
    .await
    .unwrap();
    let opened = |r: termdock_core::Result<OpenAs>| match r {
        Ok(OpenAs::Opened(fs)) => fs,
        Ok(OpenAs::PasswordRequired { retry, .. }) => panic!("password required (retry {retry})"),
        Err(err) => panic!("{err}"),
    };
    let prompted = |r: termdock_core::Result<OpenAs>, want_retry: bool| match r {
        Ok(OpenAs::PasswordRequired { pending, retry }) => {
            assert_eq!(retry, want_retry);
            pending
        }
        Ok(OpenAs::Opened(_)) => panic!("opened without a password"),
        Err(err) => panic!("{err}"),
    };

    // The login user itself: the plain subsystem, no sudo.
    let fs = opened(Sftp::open_as(&h, &e.user).await);
    assert!(fs.canonicalize(".").await.unwrap().ends_with(&format!("/{}", e.user)));

    // NOPASSWD: no prompt; starts in that user's home and creates files as that user.
    let fs = opened(Sftp::open_as(&h, &nopw).await);
    let home = fs.canonicalize(".").await.unwrap();
    assert!(home.ends_with(&format!("/{nopw}")), "{home}");
    let name = format!("td-sudo-{}", Uuid::new_v4().simple());
    fs.mkdir(&home, &name).await.unwrap();
    h.write(format!("sudo -n -u {nopw} stat -c owner-%U {home}/{name}\n").into_bytes())
        .unwrap();
    let expected = format!("owner-{nopw}");
    let owned = async {
        while !out.text.lock().unwrap().contains(&expected) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(20), owned)
        .await
        .expect("owner check");
    fs.remove(&home, &name).await.unwrap();
    fs.close().await;

    // A password is needed: sudo says whose; a wrong one goes to the same
    // sudo, which asks again; the right one opens.
    let pending = prompted(Sftp::open_as(&h, &other).await, false);
    assert_eq!(pending.asks_for(), Some(e.user.as_str()));
    let pending = prompted(Sftp::continue_as(pending, "not-the-password").await, true);
    let fs = opened(Sftp::continue_as(pending, &e.password).await);
    assert!(fs.canonicalize(".").await.unwrap().ends_with(&format!("/{other}")));
    fs.close().await;

    // sudo allows three tries, then gives up with its own message.
    let mut pending = prompted(Sftp::open_as(&h, &other).await, false);
    for _ in 0..2 {
        pending = prompted(Sftp::continue_as(pending, "wrong").await, true);
    }
    match Sftp::continue_as(pending, "wrong").await {
        Err(Error::Sudo(msg)) => assert!(msg.contains("incorrect password"), "{msg}"),
        Err(err) => panic!("{err}"),
        Ok(_) => panic!("must fail after three tries"),
    }

    // A cancelled attempt leaves nothing waiting.
    prompted(Sftp::open_as(&h, &other).await, false).cancel().await;

    // sudo does not allow root for this user: its message is reported.
    let pending = prompted(Sftp::open_as(&h, "root").await, false);
    match Sftp::continue_as(pending, &e.password).await {
        Err(Error::Sudo(msg)) => assert!(msg.contains("not allowed"), "{msg}"),
        Err(err) => panic!("{err}"),
        Ok(_) => panic!("root must be refused"),
    }

    // Names that could change the remote command never reach the server.
    for bad in ["x;id", "-u", "a b", "$(id)", "a'b", ""] {
        assert!(
            matches!(Sftp::open_as(&h, bad).await, Err(Error::InvalidName(_))),
            "{bad}"
        );
    }

    run_and_exit(h, out, "sudo").await;
}

/// Uploads streamed from memory (files dropped on the browser): exact size
/// required, nothing left behind on a short or long stream; file_info.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn sftp_streamed_upload() {
    let Some(e) = env() else {
        eprintln!("TERMDOCK_E2E_SSH not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    let vault = Arc::new(MemoryVault::default());
    let out = output();
    let h = connect(
        &e,
        SshAuth::Password,
        &kh,
        &vault,
        None,
        user(true, &[(&e.password, false)]),
        out.sink.clone(),
    )
    .await
    .unwrap();
    let fs = Sftp::open(&h).await.unwrap();
    let home = fs.canonicalize(".").await.unwrap();
    let work = format!("td-stream-{}", Uuid::new_v4().simple());
    fs.mkdir(&home, &work).await.unwrap();
    let remote_dir = sftp::join(&home, &work);
    let none = CancellationToken::new();
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();

    let n = fs
        .upload_from(
            &data[..],
            data.len() as u64,
            None,
            &remote_dir,
            "dropped.bin",
            false,
            &none,
            |_, _| {},
        )
        .await
        .unwrap();
    assert_eq!(n, data.len() as u64);
    let (size, mtime, kind) = fs.file_info(&remote_dir, "dropped.bin").await.unwrap();
    assert_eq!(size, data.len() as u64);
    assert!(mtime.is_some());
    assert_eq!(kind, sftp::FileType::File);

    // A stream shorter or longer than announced replaces nothing.
    for (bytes, announced) in [(&data[..1000], 2000u64), (&data[..3000], 2000u64)] {
        assert!(matches!(
            fs.upload_from(
                bytes,
                announced,
                None,
                &remote_dir,
                "dropped.bin",
                true,
                &none,
                |_, _| {}
            )
            .await,
            Err(Error::SizeMismatch)
        ));
    }
    let listing = fs.list(&remote_dir).await.unwrap();
    let names: Vec<_> = listing.entries.iter().map(|x| (x.name.as_str(), x.size)).collect();
    assert_eq!(
        names,
        [("dropped.bin", Some(data.len() as u64))],
        "original kept, no temporary files"
    );

    // Size caps: reading or downloading more than allowed fails.
    let read = fs
        .read_at_most(&remote_dir, "dropped.bin", data.len() as u64)
        .await
        .unwrap();
    assert_eq!(read, data);
    assert!(matches!(
        fs.read_at_most(&remote_dir, "dropped.bin", 1000).await,
        Err(Error::TooLarge)
    ));
    let local = dir.path().join("capped.bin");
    assert!(matches!(
        fs.download_at_most(&remote_dir, "dropped.bin", &local, 1000, &none, |_, _| {})
            .await,
        Err(Error::TooLarge)
    ));
    assert_eq!(
        fs.resolve(&remote_dir, "dropped.bin").await.unwrap(),
        (remote_dir.clone(), "dropped.bin".to_owned())
    );

    fs.remove(&remote_dir, "dropped.bin").await.unwrap();
    fs.remove(&home, &work).await.unwrap();
    run_and_exit(h, out, "stream").await;
}
