//! SSH sessions (russh, pure Rust).
//!
//! Security choices:
//! * Only modern algorithms are negotiated: hybrid post-quantum
//!   `mlkem768x25519-sha256` first, then curve25519; AEAD ciphers
//!   (chacha20-poly1305, AES-GCM) before AES-CTR; encrypt-then-MAC SHA-2 MACs;
//!   no SHA-1 (`ssh-rsa`, `hmac-sha1`, `diffie-hellman-*-sha1`), no CBC, no DSA,
//!   no compression. "Strict KEX" (Terrapin mitigation) is advertised.
//! * The server host key is verified before authentication (see [`crate::hostkeys`]).
//! * Secrets are held in zeroizing buffers and only persisted in the OS vault
//!   when the user ticks "remember".

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{Algorithm, EcdsaCurve, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect, MethodKind, MethodSet, Preferred, cipher, compression, kex, mac};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::forward::{self, RemoteForwards};
use crate::hostkeys::{HostKeyInfo, HostKeyStatus, KnownHosts};
use crate::profile::{Hop, SshAuth, SshTarget};
use crate::session::{
    Control, EventSink, Interaction, SecretAnswer, SecretPrompt, SessionEvent, SessionHandle, TermSize,
};
use crate::vault::{Secret, SecretKind, Vault};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// keyboard-interactive questions per attempt (OpenSSH servers send 1–2).
const MAX_INFO_REQUESTS: usize = 10;
/// Version exchange, key exchange and the host-key question (which the UI
/// expires after 3 minutes).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(4 * 60);
const MAX_PROMPTS: usize = 3;
const TERM: &str = "xterm-256color";

const KEX: &[kex::Name] = &[
    kex::MLKEM768X25519_SHA256,
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_G16_SHA512,
    kex::DH_G18_SHA512,
    kex::DH_G14_SHA256,
    kex::EXTENSION_SUPPORT_AS_CLIENT,
    kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT,
];

const CIPHERS: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_128_GCM,
    cipher::AES_256_CTR,
    cipher::AES_192_CTR,
    cipher::AES_128_CTR,
];

const MACS: &[mac::Name] = &[
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512,
    mac::HMAC_SHA256,
];

const HOST_KEYS: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP256,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP384,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP521,
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    },
];

pub fn hardened_preferred() -> Preferred {
    Preferred {
        kex: Cow::Borrowed(KEX),
        key: Cow::Borrowed(HOST_KEYS),
        host_key_certificates: Cow::Borrowed(&[]),
        cipher: Cow::Borrowed(CIPHERS),
        mac: Cow::Borrowed(MACS),
        compression: Cow::Borrowed(&[compression::NONE]),
    }
}

/// `known`: key types recorded for this host, offered first so the server
/// shows the recorded key (and any other key counts as changed).
fn client_config(target: &SshTarget, known: &[Algorithm]) -> client::Config {
    let mut preferred = hardened_preferred();
    let (first, rest): (Vec<Algorithm>, Vec<Algorithm>) = HOST_KEYS.iter().cloned().partition(|a| known.contains(a));
    preferred.key = Cow::Owned(first.into_iter().chain(rest).collect());
    client::Config {
        preferred,
        keepalive_interval: (target.keepalive_secs > 0).then(|| Duration::from_secs(target.keepalive_secs.into())),
        keepalive_max: 3,
        ..Default::default()
    }
}

/// Everything needed to open an SSH terminal.
pub struct SshConnect {
    /// Saved profile the target belongs to; `None` for quick connect (then
    /// nothing is ever stored in the vault).
    pub profile_id: Option<Uuid>,
    pub target: SshTarget,
    /// Bastions to go through, outermost first (see [`crate::profile::resolve_jumps`]).
    pub jumps: Vec<Hop>,
    pub size: TermSize,
    pub known_hosts: Arc<KnownHosts>,
    pub vault: Arc<dyn Vault>,
    pub ui: Arc<dyn Interaction>,
}

pub(crate) type SshHandle = Arc<Handle<ClientHandler>>;

pub(crate) struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    ui: Arc<dyn Interaction>,
    /// Why the host key was refused, reported after the handshake fails.
    refusal: Arc<Mutex<Option<Error>>>,
    /// `-R` forwards this connection asked the server for.
    remote_forwards: RemoteForwards,
}

impl ClientHandler {
    fn refuse(&self, err: Error) -> Result<bool, russh::Error> {
        warn!("refusing host key for {}:{}: {err}", self.host, self.port);
        *self.refusal.lock().unwrap() = Some(err);
        Ok(false)
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            return self.refuse(Error::HostKeyRejected(format!(
                "{} (host certificates are not supported)",
                self.host
            )));
        };
        let (known, host, port, checked) = (self.known_hosts.clone(), self.host.clone(), self.port, key.clone());
        let status = blocking(move || known.check(&host, port, &checked))
            .await
            .and_then(|r| r);
        match status {
            Ok(HostKeyStatus::Trusted) => Ok(true),
            Ok(HostKeyStatus::Unknown) => {
                let info = HostKeyInfo::new(&self.host, self.port, key);
                if self.ui.confirm_host_key(info).await {
                    let (known, host, port, learned) =
                        (self.known_hosts.clone(), self.host.clone(), self.port, key.clone());
                    if let Err(e) = blocking(move || known.learn(&host, port, &learned))
                        .await
                        .and_then(|r| r)
                    {
                        warn!("could not record host key: {e}");
                    }
                    Ok(true)
                } else {
                    self.refuse(Error::HostKeyRejected(self.host.clone()))
                }
            }
            Ok(HostKeyStatus::Changed { file, line }) | Ok(HostKeyStatus::Revoked { file, line }) => {
                self.refuse(Error::HostKeyChanged {
                    host: format!("{}:{} ({})", self.host, self.port, file.display()),
                    line,
                })
            }
            Err(e) => self.refuse(e),
        }
    }

    async fn auth_banner(&mut self, banner: &str, _session: &mut client::Session) -> Result<(), Self::Error> {
        self.ui.banner(banner.to_owned());
        Ok(())
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        connected_address: &str,
        connected_port: u32,
        originator_address: &str,
        originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        match self.remote_forwards.lookup(connected_address, connected_port) {
            Some((host, port)) => {
                debug!(
                    "remote forward {connected_address}:{connected_port} from {originator_address}:{originator_port}"
                );
                forward::serve_remote(channel, reply, host, port);
            }
            // Never requested by us: refuse instead of connecting anywhere.
            None => prohibited(reply).await,
        }
        Ok(())
    }

    // Every other channel a server may open is refused: Termdock never asks
    // for X11, agent or socket forwarding, and a server has no business
    // opening sessions or tunnels into the client.

    async fn server_channel_open_x11(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        prohibited(reply).await;
        Ok(())
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        prohibited(reply).await;
        Ok(())
    }

    async fn server_channel_open_session(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        prohibited(reply).await;
        Ok(())
    }

    async fn server_channel_open_direct_tcpip(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        prohibited(reply).await;
        Ok(())
    }

    async fn server_channel_open_forwarded_streamlocal(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _socket_path: &str,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        prohibited(reply).await;
        Ok(())
    }

    async fn server_channel_open_direct_streamlocal(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _socket_path: &str,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        prohibited(reply).await;
        Ok(())
    }
}

/// Runs blocking work (the OS keychain, known_hosts files, key decryption)
/// on the blocking pool, so a slow disk or a keychain waiting for its unlock
/// dialog does not stall the other sessions on the async runtime.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(work).await.map_err(|e| {
        if e.is_panic() {
            std::panic::resume_unwind(e.into_panic());
        }
        Error::Cancelled
    })
}

/// Decodes a private key; with a passphrase this runs a deliberately slow
/// key derivation (bcrypt-pbkdf), hence off the async runtime.
async fn decode_key(
    text: &Zeroizing<String>,
    passphrase: Option<Secret>,
) -> Result<std::result::Result<PrivateKey, russh::keys::Error>> {
    let text = text.clone();
    blocking(move || russh::keys::decode_secret_key(&text, passphrase.as_deref().map(String::as_str))).await
}

async fn prohibited(reply: client::ChannelOpenHandle) {
    reply
        .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
        .await
}

struct Context {
    known_hosts: Arc<KnownHosts>,
    vault: Arc<dyn Vault>,
    ui: Arc<dyn Interaction>,
}

/// SSH handshake, host key check and authentication of one hop over any
/// transport: a TCP socket, or a `direct-tcpip` channel of the previous hop.
async fn establish<S>(
    hop: &Hop,
    stream: S,
    ctx: &Context,
    remote_forwards: RemoteForwards,
) -> Result<Handle<ClientHandler>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let target = &hop.target;
    let refusal = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        host: target.host.clone(),
        port: target.port,
        known_hosts: ctx.known_hosts.clone(),
        ui: ctx.ui.clone(),
        refusal: refusal.clone(),
        remote_forwards,
    };
    let known = {
        let (known_hosts, host, port) = (ctx.known_hosts.clone(), target.host.clone(), target.port);
        blocking(move || known_hosts.known_algorithms(&host, port)).await?
    };
    let config = Arc::new(client_config(target, &known));
    // The handshake has no timeout of its own: a port that accepts and stays
    // silent would hang here forever. The bound leaves time for the host-key
    // question.
    let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, client::connect_stream(config, stream, handler)).await;
    let mut handle = match handshake {
        Err(_) => return Err(Error::Timeout),
        Ok(Ok(h)) => h,
        Ok(Err(e)) => return Err(refusal.lock().unwrap().take().unwrap_or(Error::Ssh(e))),
    };

    let authenticated = Authenticator {
        handle: &mut handle,
        target,
        profile_id: hop.profile_id,
        vault: ctx.vault.clone(),
        ui: ctx.ui.as_ref(),
        remaining: None,
        partial: false,
        kbd_tried: false,
    }
    .run()
    .await;
    match authenticated {
        Ok(true) => {
            info!("authenticated to {}", target.display());
            Ok(handle)
        }
        Ok(false) => {
            let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
            Err(Error::AuthFailed(target.display()))
        }
        Err(e) => {
            let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
            Err(e)
        }
    }
}

async fn disconnect_all(handles: &[SshHandle]) {
    // Innermost first, so each tunnel closes before the one carrying it.
    for h in handles.iter().rev() {
        let _ = h.disconnect(Disconnect::ByApplication, "", "en").await;
    }
}

/// Connects through every jump host, then to the target.
async fn connect_chain(hops: &[Hop], ctx: &Context, remote_forwards: RemoteForwards) -> Result<Vec<SshHandle>> {
    let mut handles: Vec<SshHandle> = Vec::with_capacity(hops.len());
    for (i, hop) in hops.iter().enumerate() {
        let last = i + 1 == hops.len();
        let forwards = if last {
            remote_forwards.clone()
        } else {
            RemoteForwards::default()
        };
        let result = match handles.last() {
            None => {
                info!("connecting to {}", hop.target.display());
                let addr = (hop.target.host.as_str(), hop.target.port);
                match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
                    Err(_) => Err(Error::Timeout),
                    Ok(Err(e)) => Err(e.into()),
                    Ok(Ok(stream)) => {
                        stream.set_nodelay(true)?;
                        establish(hop, stream, ctx, forwards).await
                    }
                }
            }
            Some(via) => {
                let from = &hops[i - 1].target;
                ctx.ui
                    .info(format!("jump: {} -> {}", from.display(), hop.target.display()));
                let channel = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    via.channel_open_direct_tcpip(hop.target.host.clone(), hop.target.port.into(), "127.0.0.1", 0),
                )
                .await;
                match channel {
                    Err(_) => Err(Error::Timeout),
                    Ok(Err(e)) => Err(Error::Jump(format!(
                        "{} could not open a tunnel to {}:{}: {e}",
                        from.display(),
                        hop.target.host,
                        hop.target.port
                    ))),
                    Ok(Ok(channel)) => establish(hop, channel.into_stream(), ctx, forwards).await,
                }
            }
        };
        match result {
            Ok(handle) => handles.push(Arc::new(handle)),
            Err(e) => {
                disconnect_all(&handles).await;
                return Err(e);
            }
        }
    }
    Ok(handles)
}

pub async fn connect(params: SshConnect, sink: EventSink) -> Result<SessionHandle> {
    let SshConnect {
        profile_id,
        target,
        jumps,
        size,
        known_hosts,
        vault,
        ui,
    } = params;
    let ctx = Context {
        known_hosts,
        vault,
        ui: ui.clone(),
    };
    let forwards = target.forwards.clone();
    let login_user = target.username.clone();
    let mut hops = jumps;
    hops.push(Hop { profile_id, target });

    let remote_forwards = RemoteForwards::default();
    let handles = connect_chain(&hops, &ctx, remote_forwards.clone()).await?;
    let handle = handles.last().expect("at least the target").clone();

    let opened = async {
        let channel = handle.channel_open_session().await?;
        let size = size.clamped();
        channel
            .request_pty(false, TERM, size.cols.into(), size.rows.into(), 0, 0, &[])
            .await?;
        channel.request_shell(false).await?;
        Ok::<_, Error>(channel)
    }
    .await;
    let channel = match opened {
        Ok(c) => c,
        Err(e) => {
            disconnect_all(&handles).await;
            return Err(e);
        }
    };

    let forward_tasks = forward::start(&handle, &forwards, &ui, &remote_forwards).await;

    let (mut read_half, write_half) = channel.split();
    let (session, mut rx) = SessionHandle::new(Some((handle.clone(), login_user)));

    // Set when the session is closed on purpose, so the end is not taken
    // for a dropped connection.
    let closing = Arc::new(AtomicBool::new(false));

    // Input side.
    let writer_handle = handle.clone();
    let closed_here = closing.clone();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Some(Control::Input(data)) => {
                    if write_half.data(&data[..]).await.is_err() {
                        break;
                    }
                }
                Some(Control::Resize(s)) => {
                    let _ = write_half.window_change(s.cols.into(), s.rows.into(), 0, 0).await;
                }
                // Closed, or every handle to the session dropped.
                Some(Control::Close) | None => {
                    closed_here.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        let _ = write_half.close().await;
        let _ = writer_handle
            .disconnect(Disconnect::ByApplication, "session closed", "en")
            .await;
    });

    // Output side; also owns the connection chain and the forwards.
    tokio::spawn(async move {
        let mut code = None;
        let mut message = None;
        let mut channel_closed = false;
        while let Some(msg) = read_half.wait().await {
            match msg {
                ChannelMsg::Data { data } => sink(SessionEvent::Data(data.to_vec())),
                ChannelMsg::ExtendedData { data, .. } => sink(SessionEvent::Data(data.to_vec())),
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                ChannelMsg::ExitSignal {
                    signal_name,
                    error_message,
                    ..
                } => {
                    message = Some(
                        format!("killed by signal {signal_name:?} {error_message}")
                            .trim()
                            .to_owned(),
                    )
                }
                ChannelMsg::Close => {
                    channel_closed = true;
                    break;
                }
                _ => {}
            }
        }
        // The server closes the channel when the shell ends, with its exit
        // status; a connection that just stops (keepalive unanswered, reset,
        // server gone) ends without either.
        let lost = !channel_closed && code.is_none() && message.is_none() && !closing.load(Ordering::SeqCst);
        for task in forward_tasks {
            task.abort();
        }
        disconnect_all(&handles).await;
        sink(SessionEvent::Exit { code, message, lost });
    });

    Ok(session)
}

struct Authenticator<'a> {
    handle: &'a mut Handle<ClientHandler>,
    target: &'a SshTarget,
    profile_id: Option<Uuid>,
    vault: Arc<dyn Vault>,
    ui: &'a dyn Interaction,
    /// Methods the server still accepts; `None` until the first answer.
    remaining: Option<MethodSet>,
    /// The last failure was a partial success (one factor accepted).
    partial: bool,
    /// keyboard-interactive was already used (for a password).
    kbd_tried: bool,
}

impl Authenticator<'_> {
    async fn run(mut self) -> Result<bool> {
        let user = self.target.username.clone();
        let none = self.handle.authenticate_none(&user).await?;
        if self.record(none) {
            return Ok(true);
        }

        let done = match self.target.auth.clone() {
            SshAuth::Agent => self.try_agent().await?,
            SshAuth::Key { path } => self.try_key_file(&path, true).await?,
            SshAuth::Password => self.try_password().await?,
            SshAuth::Auto => self.try_agent().await? || self.try_default_keys().await? || self.try_password().await?,
        };
        if done {
            return Ok(true);
        }
        // Second factor after a partial success (e.g. key + one-time code).
        // Not a second round of a password already refused over
        // keyboard-interactive: that would only add failed attempts
        // (MaxAuthTries, fail2ban).
        if self.allows(MethodKind::KeyboardInteractive) && self.remaining.is_some() && (self.partial || !self.kbd_tried)
        {
            return self.try_keyboard_interactive(None).await;
        }
        Ok(false)
    }

    fn allows(&self, method: MethodKind) -> bool {
        self.remaining.as_ref().is_none_or(|r| r.contains(&method))
    }

    fn record(&mut self, result: AuthResult) -> bool {
        match result {
            AuthResult::Success => true,
            AuthResult::Failure {
                remaining_methods,
                partial_success,
            } => {
                self.remaining = Some(remaining_methods);
                self.partial = partial_success;
                false
            }
        }
    }

    fn can_remember(&self) -> bool {
        self.profile_id.is_some()
    }

    async fn vault_get(&self, kind: SecretKind) -> Option<Secret> {
        let id = self.profile_id?;
        let vault = self.vault.clone();
        blocking(move || vault.get(id, kind))
            .await
            .and_then(|r| r)
            .unwrap_or_else(|e| {
                warn!("vault read failed: {e}");
                None
            })
    }

    /// A remembered secret the server (or the key file) rejected: forget it so
    /// it is not replayed on every login, which could lock the account
    /// (MaxAuthTries, fail2ban).
    async fn vault_forget(&self, kind: SecretKind) {
        if let Some(id) = self.profile_id {
            let vault = self.vault.clone();
            match blocking(move || vault.delete(id, kind)).await.and_then(|r| r) {
                Ok(()) => self.ui.info(format!(
                    "the saved {} was rejected and has been removed",
                    match kind {
                        SecretKind::Password => "password",
                        SecretKind::KeyPassphrase => "key passphrase",
                    }
                )),
                Err(e) => warn!("vault delete failed: {e}"),
            }
        }
    }

    async fn vault_store(&self, kind: SecretKind, answer: &SecretAnswer) {
        let Some(id) = self.profile_id.filter(|_| answer.remember) else {
            return;
        };
        let (vault, secret) = (self.vault.clone(), answer.secret.clone());
        if let Err(e) = blocking(move || vault.set(id, kind, &secret)).await.and_then(|r| r) {
            warn!("vault write failed: {e}");
        }
    }

    /// The hash an RSA key signs with: always SHA-2, never SHA-1 (`ssh-rsa`).
    /// `Some(None)` for other keys; `None` when the server only takes SHA-1
    /// RSA signatures, so the key is not used.
    async fn rsa_hash(&mut self, alg: &Algorithm) -> Result<Option<Option<HashAlg>>> {
        if !alg.clone().is_rsa() {
            return Ok(Some(None));
        }
        Ok(match self.handle.best_supported_rsa_hash().await? {
            // The server did not say (no server-sig-algs): SHA-512 is the
            // best guess, as the russh documentation suggests.
            None => Some(Some(HashAlg::Sha512)),
            Some(Some(hash)) => Some(Some(hash)),
            Some(None) => {
                debug!("skipping RSA key: the server only accepts SHA-1 signatures");
                None
            }
        })
    }

    async fn try_agent(&mut self) -> Result<bool> {
        if !self.allows(MethodKind::PublicKey) {
            return Ok(false);
        }
        let Some(mut agent) = connect_agent().await else {
            debug!("no SSH agent available");
            return Ok(false);
        };
        let identities = agent.request_identities().await.unwrap_or_default();
        for identity in identities {
            let AgentIdentity::PublicKey { key, .. } = identity else {
                continue;
            };
            let Some(hash) = self.rsa_hash(&key.algorithm()).await? else {
                continue;
            };
            let user = self.target.username.clone();
            match self
                .handle
                .authenticate_publickey_with(user, key, hash, &mut agent)
                .await
            {
                Ok(r) => {
                    if self.record(r) {
                        return Ok(true);
                    }
                }
                Err(e) => warn!("agent signing failed: {e}"),
            }
            if !self.allows(MethodKind::PublicKey) {
                break;
            }
        }
        Ok(false)
    }

    async fn try_default_keys(&mut self) -> Result<bool> {
        let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().join(".ssh")) else {
            return Ok(false);
        };
        for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
            let path = home.join(name);
            let is_file = tokio::fs::metadata(&path).await.is_ok_and(|m| m.is_file());
            if is_file && self.try_key_file(&path, false).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `explicit`: the user chose this key, so errors are reported instead of
    /// silently skipping it.
    async fn try_key_file(&mut self, path: &Path, explicit: bool) -> Result<bool> {
        if !self.allows(MethodKind::PublicKey) {
            return Ok(false);
        }
        let path = expand_home(path);
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(t) => Zeroizing::<String>::new(t),
            Err(e) if !explicit => {
                debug!("skipping key {}: {e}", path.display());
                return Ok(false);
            }
            Err(e) => return Err(e.into()),
        };
        let key = match decode_key(&text, None).await? {
            Ok(k) => k,
            Err(russh::keys::Error::KeyIsEncrypted) => match self.unlock_key(&text, &path).await? {
                Some(k) => k,
                None if explicit => return Err(Error::AuthCancelled),
                None => return Ok(false),
            },
            Err(e) if explicit => return Err(e.into()),
            Err(e) => {
                debug!("skipping key {}: {e}", path.display());
                return Ok(false);
            }
        };
        let Some(hash) = self.rsa_hash(&key.algorithm()).await? else {
            return Ok(false);
        };
        let user = self.target.username.clone();
        let r = self
            .handle
            .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
            .await?;
        Ok(self.record(r))
    }

    async fn unlock_key(&mut self, text: &Zeroizing<String>, path: &Path) -> Result<Option<PrivateKey>> {
        if let Some(saved) = self.vault_get(SecretKind::KeyPassphrase).await {
            match decode_key(text, Some(saved)).await? {
                Ok(k) => return Ok(Some(k)),
                Err(_) => self.vault_forget(SecretKind::KeyPassphrase).await,
            }
        }
        for _ in 0..MAX_PROMPTS {
            let prompt = SecretPrompt::KeyPassphrase {
                path: path.to_owned(),
                can_remember: self.can_remember(),
            };
            let Some(answer) = self.ui.ask_secret(prompt).await else {
                return Ok(None);
            };
            if let Ok(k) = decode_key(text, Some(answer.secret.clone())).await? {
                self.vault_store(SecretKind::KeyPassphrase, &answer).await;
                return Ok(Some(k));
            }
        }
        Ok(None)
    }

    async fn try_password(&mut self) -> Result<bool> {
        if !self.allows(MethodKind::Password) {
            if self.allows(MethodKind::KeyboardInteractive) {
                let saved = self.vault_get(SecretKind::Password).await;
                return self.try_keyboard_interactive(saved).await;
            }
            return Ok(false);
        }
        let user = self.target.username.clone();
        if let Some(saved) = self.vault_get(SecretKind::Password).await {
            let r = self.handle.authenticate_password(&user, saved.as_str()).await?;
            if self.record(r) {
                return Ok(true);
            }
            self.vault_forget(SecretKind::Password).await;
        }
        for _ in 0..MAX_PROMPTS {
            if !self.allows(MethodKind::Password) {
                break;
            }
            let prompt = SecretPrompt::Password {
                target: self.target.display(),
                can_remember: self.can_remember(),
            };
            let Some(answer) = self.ui.ask_secret(prompt).await else {
                return Err(Error::AuthCancelled);
            };
            let r = self.handle.authenticate_password(&user, answer.secret.as_str()).await?;
            if self.record(r) {
                self.vault_store(SecretKind::Password, &answer).await;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// keyboard-interactive: used for passwords on servers that only offer
    /// it, and for one-time codes / second factors.
    async fn try_keyboard_interactive(&mut self, mut saved_password: Option<Secret>) -> Result<bool> {
        self.kbd_tried = true;
        let user = self.target.username.clone();
        for _ in 0..MAX_PROMPTS {
            if !self.allows(MethodKind::KeyboardInteractive) {
                return Ok(false);
            }
            let mut remember: Option<SecretAnswer> = None;
            let mut used_saved = false;
            let mut reply = self.handle.authenticate_keyboard_interactive_start(&user, None).await?;
            // A server could keep sending questions (even empty ones) forever.
            let mut requests = 0;
            loop {
                match reply {
                    KeyboardInteractiveAuthResponse::Success => {
                        if let Some(a) = &remember {
                            self.vault_store(SecretKind::Password, a).await;
                        }
                        return Ok(true);
                    }
                    KeyboardInteractiveAuthResponse::Failure {
                        remaining_methods,
                        partial_success,
                    } => {
                        self.remaining = Some(remaining_methods);
                        self.partial = partial_success;
                        if used_saved {
                            self.vault_forget(SecretKind::Password).await;
                        }
                        break;
                    }
                    KeyboardInteractiveAuthResponse::InfoRequest {
                        name,
                        instructions,
                        prompts,
                    } => {
                        requests += 1;
                        if requests > MAX_INFO_REQUESTS {
                            return Ok(false);
                        }
                        let mut responses = Vec::with_capacity(prompts.len());
                        for p in &prompts {
                            let looks_like_password = !p.echo && p.prompt.to_lowercase().contains("password");
                            if looks_like_password && let Some(pw) = saved_password.take() {
                                responses.push(pw.to_string());
                                used_saved = true;
                                continue;
                            }
                            let prompt = SecretPrompt::Challenge {
                                target: self.target.display(),
                                name: name.clone(),
                                instructions: instructions.clone(),
                                prompt: p.prompt.clone(),
                                echo: p.echo,
                                can_remember: looks_like_password && self.can_remember(),
                            };
                            let Some(answer) = self.ui.ask_secret(prompt).await else {
                                return Err(Error::AuthCancelled);
                            };
                            responses.push(answer.secret.to_string());
                            if looks_like_password {
                                remember = Some(answer);
                            }
                        }
                        reply = self.handle.authenticate_keyboard_interactive_respond(responses).await?;
                    }
                }
            }
        }
        Ok(false)
    }
}

type DynAgent = AgentClient<Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>>;

#[cfg(unix)]
async fn connect_agent() -> Option<DynAgent> {
    AgentClient::connect_env().await.ok().map(AgentClient::dynamic)
}

#[cfg(windows)]
async fn connect_agent() -> Option<DynAgent> {
    // Windows OpenSSH agent service first, then PuTTY's Pageant.
    if let Ok(a) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        return Some(a.dynamic());
    }
    AgentClient::connect_pageant().await.ok().map(AgentClient::dynamic)
}

fn expand_home(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(base) = directories::BaseDirs::new()
    {
        return base.home_dir().join(rest);
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_weak_algorithms() {
        let p = hardened_preferred();
        for k in p.kex.iter() {
            let name: &str = k.as_ref();
            assert!(!name.ends_with("sha1"), "{name}");
        }
        for c in p.cipher.iter() {
            let name: &str = c.as_ref();
            assert!(!name.contains("cbc") && name != "none", "{name}");
        }
        for m in p.mac.iter() {
            let name: &str = m.as_ref();
            assert!(!name.contains("sha1") && name != "none", "{name}");
        }
        assert!(!p.key.contains(&Algorithm::Rsa { hash: None }));
        assert!(!p.key.contains(&Algorithm::Dsa));
        assert_eq!(p.kex[0], kex::MLKEM768X25519_SHA256);
        assert!(p.kex.contains(&kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT));
    }

    #[test]
    fn home_expansion() {
        let p = expand_home(Path::new("~/.ssh/id_ed25519"));
        assert!(!p.starts_with("~"));
        assert_eq!(expand_home(Path::new("/abs/key")), PathBuf::from("/abs/key"));
    }
}
