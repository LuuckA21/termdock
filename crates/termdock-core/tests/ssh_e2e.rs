//! End-to-end tests against a real OpenSSH server.
//!
//! Skipped unless `TERMDOCK_E2E_SSH` is set; see `scripts/e2e-sshd.sh`, which
//! starts a throw-away sshd and exports the variables used here:
//!
//! * `TERMDOCK_E2E_SSH`      `host:port`
//! * `TERMDOCK_E2E_USER`     login user
//! * `TERMDOCK_E2E_PASSWORD` its password
//! * `TERMDOCK_E2E_KEY`      private key (passphrase `TERMDOCK_E2E_KEY_PASS`)

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use termdock_core::Error;
use termdock_core::hostkeys::{HostKeyInfo, KnownHosts};
use termdock_core::profile::{Forward, Hop, SshAuth, SshTarget};
use termdock_core::session::{
    BoxFuture, EventSink, Interaction, SecretAnswer, SecretPrompt, SessionEvent, SessionHandle, TermSize,
};
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
