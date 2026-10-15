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
use termdock_core::profile::{SshAuth, SshTarget};
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
            },
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
