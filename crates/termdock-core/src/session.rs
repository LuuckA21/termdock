//! Transport independent terminal session plumbing.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::hostkeys::HostKeyInfo;
use crate::vault::Secret;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl TermSize {
    pub fn clamped(self) -> Self {
        Self {
            cols: self.cols.clamp(2, 1000),
            rows: self.rows.clamp(1, 500),
        }
    }
}

impl Default for TermSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// Output of a session, delivered in order.
#[derive(Debug)]
pub enum SessionEvent {
    Data(Vec<u8>),
    /// The session ended. Sent exactly once, after the last `Data`.
    Exit {
        code: Option<u32>,
        message: Option<String>,
    },
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

#[derive(Debug)]
pub(crate) enum Control {
    Input(Vec<u8>),
    Resize(TermSize),
    Close,
}

/// Cheap, cloneable handle used to drive a running session. All methods are
/// non-blocking: requests are queued to the session's own task/thread.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    tx: mpsc::UnboundedSender<Control>,
}

impl SessionHandle {
    pub(crate) fn new() -> (Self, mpsc::UnboundedReceiver<Control>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    pub fn write(&self, data: Vec<u8>) -> Result<()> {
        self.tx.send(Control::Input(data)).map_err(|_| Error::SessionClosed)
    }

    pub fn resize(&self, size: TermSize) -> Result<()> {
        self.tx
            .send(Control::Resize(size.clamped()))
            .map_err(|_| Error::SessionClosed)
    }

    pub fn close(&self) {
        let _ = self.tx.send(Control::Close);
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// A question the backend needs the user to answer while connecting.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SecretPrompt {
    #[serde(rename_all = "camelCase")]
    Password { target: String, can_remember: bool },
    #[serde(rename_all = "camelCase")]
    KeyPassphrase { path: PathBuf, can_remember: bool },
    /// keyboard-interactive challenge, e.g. a one-time code.
    #[serde(rename_all = "camelCase")]
    Challenge {
        target: String,
        name: String,
        instructions: String,
        prompt: String,
        echo: bool,
        can_remember: bool,
    },
}

pub struct SecretAnswer {
    pub secret: Secret,
    /// Store it in the OS vault (only honoured when the prompt allows it).
    pub remember: bool,
}

/// Implemented by the UI layer.
pub trait Interaction: Send + Sync {
    /// Ask whether to trust a host key seen for the first time.
    fn confirm_host_key(&self, info: HostKeyInfo) -> BoxFuture<'_, bool>;

    /// Ask for a secret. `None` means the user cancelled.
    fn ask_secret(&self, prompt: SecretPrompt) -> BoxFuture<'_, Option<SecretAnswer>>;

    /// Pre-authentication banner sent by the server.
    fn banner(&self, _text: String) {}
}
