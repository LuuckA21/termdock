//! Port forwarding over an authenticated SSH connection: local (`-L`),
//! remote (`-R`) and dynamic SOCKS5 (`-D`).
//!
//! Listeners live as long as the terminal session; their tasks are aborted
//! when it ends. Local listeners bind to the address configured in the profile
//! (loopback by default), exactly like OpenSSH.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::ChannelOpenFailure;
use russh::client::{ChannelOpenHandle, Msg};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;
use tracing::debug;

use crate::profile::Forward;
use crate::session::Interaction;
use crate::ssh::SshHandle;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// (bind address, port) as requested on the server.
type RemoteBind = (String, u32);
/// (host, port) to connect to from this machine.
type LocalDest = (String, u16);

/// `-R` forwards requested on the server.
#[derive(Clone, Default)]
pub(crate) struct RemoteForwards(Arc<Mutex<HashMap<RemoteBind, LocalDest>>>);

impl RemoteForwards {
    fn insert(&self, bind: RemoteBind, dest: LocalDest) {
        self.0.lock().unwrap().insert(bind, dest);
    }

    fn remove(&self, bind: &RemoteBind) {
        self.0.lock().unwrap().remove(bind);
    }

    /// Destination for a connection the server opened on `address:port`.
    /// Servers normally echo the requested bind address; if one rewrites it,
    /// fall back to the port when that is unambiguous.
    pub(crate) fn lookup(&self, address: &str, port: u32) -> Option<LocalDest> {
        let map = self.0.lock().unwrap();
        if let Some(dest) = map.get(&(address.to_owned(), port)) {
            return Some(dest.clone());
        }
        let mut by_port = map.iter().filter(|((_, p), _)| *p == port);
        match (by_port.next(), by_port.next()) {
            (Some((_, dest)), None) => Some(dest.clone()),
            _ => None,
        }
    }
}

/// Opens every forward of the profile. Failures are reported to the user
/// and do not end the session.
pub(crate) async fn start(
    handle: &SshHandle,
    forwards: &[Forward],
    ui: &Arc<dyn Interaction>,
    remote: &RemoteForwards,
) -> Vec<AbortHandle> {
    let mut tasks = Vec::new();
    for forward in forwards {
        let label = forward.describe();
        let result = match forward.clone() {
            Forward::Local {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => match TcpListener::bind((bind_host.as_str(), bind_port)).await {
                Ok(listener) => {
                    tasks.push(spawn_local(listener, handle.clone(), Some((dest_host, dest_port))));
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            },
            Forward::Dynamic { bind_host, bind_port } => match TcpListener::bind((bind_host.as_str(), bind_port)).await
            {
                Ok(listener) => {
                    tasks.push(spawn_local(listener, handle.clone(), None));
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            },
            Forward::Remote {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => {
                let key = (bind_host.clone(), u32::from(bind_port));
                remote.insert(key.clone(), (dest_host, dest_port));
                match handle.tcpip_forward(bind_host, bind_port.into()).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        remote.remove(&key);
                        Err(match e {
                            russh::Error::RequestDenied => "refused by the server (AllowTcpForwarding?)".into(),
                            e => e.to_string(),
                        })
                    }
                }
            }
        };
        ui.info(match result {
            Ok(()) => format!("forward {label}: active"),
            Err(e) => format!("forward {label}: FAILED ({e})"),
        });
    }
    tasks
}

/// Accept loop of a `-L` (fixed destination) or `-D` (SOCKS5) listener.
fn spawn_local(listener: TcpListener, handle: SshHandle, dest: Option<LocalDest>) -> AbortHandle {
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, peer)) = listener.accept().await else {
                continue;
            };
            let handle = handle.clone();
            let dest = dest.clone();
            tokio::spawn(async move {
                let _ = tcp.set_nodelay(true);
                let (host, port, socks) = match dest {
                    Some((h, p)) => (h, p, false),
                    None => match socks5_accept(&mut tcp).await {
                        Ok((h, p)) => (h, p, true),
                        Err(e) => {
                            debug!("SOCKS request from {peer} rejected: {e}");
                            return;
                        }
                    },
                };
                let channel = handle
                    .channel_open_direct_tcpip(host.clone(), port.into(), peer.ip().to_string(), peer.port().into())
                    .await;
                let channel = match channel {
                    Ok(c) => c,
                    Err(e) => {
                        debug!("forward to {host}:{port} failed: {e}");
                        if socks {
                            let _ = socks5_reply(&mut tcp, REP_HOST_UNREACHABLE).await;
                        }
                        return;
                    }
                };
                if socks && socks5_reply(&mut tcp, REP_SUCCEEDED).await.is_err() {
                    return;
                }
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
            });
        }
    })
    .abort_handle()
}

/// Serves a connection the server opened for a `-R` forward.
pub(crate) fn serve_remote(channel: russh::Channel<Msg>, reply: ChannelOpenHandle, host: String, port: u16) {
    tokio::spawn(async move {
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port))).await;
        match tcp {
            Ok(Ok(mut tcp)) => {
                reply.accept().await;
                let _ = tcp.set_nodelay(true);
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
            }
            _ => {
                debug!("remote forward: cannot reach {host}:{port}");
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
            }
        }
    });
}

// --- SOCKS5 (RFC 1928), CONNECT only, no authentication -------------------

const SOCKS_VERSION: u8 = 5;
const METHOD_NO_AUTH: u8 = 0;
const METHOD_NONE_ACCEPTABLE: u8 = 0xff;
const CMD_CONNECT: u8 = 1;
const ATYP_IPV4: u8 = 1;
const ATYP_DOMAIN: u8 = 3;
const ATYP_IPV6: u8 = 4;
const REP_SUCCEEDED: u8 = 0;
const REP_HOST_UNREACHABLE: u8 = 4;
const REP_COMMAND_NOT_SUPPORTED: u8 = 7;
const REP_ADDRESS_NOT_SUPPORTED: u8 = 8;

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}

/// Reads the SOCKS5 greeting and CONNECT request, returning the destination.
/// On success the caller must answer with [`socks5_reply`].
async fn socks5_accept<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S) -> io::Result<(String, u16)> {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await?;
    if head[0] != SOCKS_VERSION {
        return Err(invalid("not a SOCKS5 client"));
    }
    let mut methods = vec![0u8; head[1].into()];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&METHOD_NO_AUTH) {
        s.write_all(&[SOCKS_VERSION, METHOD_NONE_ACCEPTABLE]).await?;
        return Err(invalid("client requires authentication"));
    }
    s.write_all(&[SOCKS_VERSION, METHOD_NO_AUTH]).await?;

    let mut req = [0u8; 4];
    s.read_exact(&mut req).await?;
    if req[0] != SOCKS_VERSION {
        return Err(invalid("bad request version"));
    }
    let host = match req[3] {
        ATYP_IPV4 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        ATYP_IPV6 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            Ipv6Addr::from(a).to_string()
        }
        ATYP_DOMAIN => {
            let len = s.read_u8().await?;
            let mut name = vec![0u8; len.into()];
            s.read_exact(&mut name).await?;
            match String::from_utf8(name) {
                Ok(n) if !n.is_empty() && !n.chars().any(|c| c.is_control() || c.is_whitespace()) => n,
                _ => {
                    socks5_reply(s, REP_ADDRESS_NOT_SUPPORTED).await?;
                    return Err(invalid("bad domain name"));
                }
            }
        }
        _ => {
            socks5_reply(s, REP_ADDRESS_NOT_SUPPORTED).await?;
            return Err(invalid("unknown address type"));
        }
    };
    let port = s.read_u16().await?;
    if req[1] != CMD_CONNECT {
        socks5_reply(s, REP_COMMAND_NOT_SUPPORTED).await?;
        return Err(invalid("only CONNECT is supported"));
    }
    Ok((host, port))
}

async fn socks5_reply<S: AsyncWrite + Unpin>(s: &mut S, rep: u8) -> io::Result<()> {
    // The bound address is not meaningful here; report 0.0.0.0:0.
    s.write_all(&[SOCKS_VERSION, rep, 0, ATYP_IPV4, 0, 0, 0, 0, 0, 0])
        .await?;
    s.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn run(client_bytes: Vec<u8>) -> (io::Result<(String, u16)>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(&client_bytes).await.unwrap();
        let res = socks5_accept(&mut server).await;
        drop(server);
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).await.unwrap();
        (res, answer)
    }

    #[tokio::test]
    async fn socks_domain() {
        let mut req = vec![5, 1, 0, 5, 1, 0, 3, 11];
        req.extend(b"example.com");
        req.extend(443u16.to_be_bytes());
        let (res, answer) = run(req).await;
        assert_eq!(res.unwrap(), ("example.com".into(), 443));
        assert_eq!(answer, [5, 0]);
    }

    #[tokio::test]
    async fn socks_ip_addresses() {
        let (res, _) = run(vec![5, 2, 2, 0, 5, 1, 0, 1, 10, 0, 0, 7, 0, 22]).await;
        assert_eq!(res.unwrap(), ("10.0.0.7".into(), 22));

        let mut req = vec![5, 1, 0, 5, 1, 0, 4];
        req.extend(Ipv6Addr::LOCALHOST.octets());
        req.extend(8080u16.to_be_bytes());
        let (res, _) = run(req).await;
        assert_eq!(res.unwrap(), ("::1".into(), 8080));
    }

    #[tokio::test]
    async fn socks_rejections() {
        // Only username/password offered.
        let (res, answer) = run(vec![5, 1, 2]).await;
        assert!(res.is_err());
        assert_eq!(answer, [5, 0xff]);

        // BIND is not supported.
        let (res, answer) = run(vec![5, 1, 0, 5, 2, 0, 1, 1, 2, 3, 4, 0, 80]).await;
        assert!(res.is_err());
        assert_eq!(answer[..4], [5, 0, 5, REP_COMMAND_NOT_SUPPORTED]);

        // SOCKS4 client.
        let (res, _) = run(vec![4, 1, 0, 80, 1, 2, 3, 4, 0]).await;
        assert!(res.is_err());

        // Domain with a control character.
        let mut req = vec![5, 1, 0, 5, 1, 0, 3, 3];
        req.extend(b"a\nb");
        req.extend(80u16.to_be_bytes());
        let (res, answer) = run(req).await;
        assert!(res.is_err());
        assert_eq!(answer[..4], [5, 0, 5, REP_ADDRESS_NOT_SUPPORTED]);
    }

    #[test]
    fn remote_lookup() {
        let r = RemoteForwards::default();
        r.insert(("localhost".into(), 9000), ("127.0.0.1".into(), 3000));
        r.insert(("0.0.0.0".into(), 9001), ("127.0.0.1".into(), 3001));
        assert_eq!(r.lookup("localhost", 9000), Some(("127.0.0.1".into(), 3000)));
        assert_eq!(r.lookup("127.0.0.1", 9000), Some(("127.0.0.1".into(), 3000)));
        assert_eq!(r.lookup("localhost", 1234), None);
        r.insert(("127.0.0.1".into(), 9000), ("127.0.0.1".into(), 4000));
        // Ambiguous by port: only the exact match is accepted.
        assert_eq!(r.lookup("::1", 9000), None);
    }
}
