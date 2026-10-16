//! Bridges questions from the SSH layer (trust this host key? password?) to
//! the UI and waits for the answer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use termdock_core::hostkeys::HostKeyInfo;
use termdock_core::session::{BoxFuture, Interaction, SecretAnswer, SecretPrompt};
use tokio::sync::oneshot;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// A prompt that is not answered in time is treated as "no".
const PROMPT_TIMEOUT: Duration = Duration::from_secs(180);

/// Answer coming from the UI.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PromptAnswer {
    HostKey { accept: bool },
    Secret { secret: String, remember: bool },
    Cancel,
}

impl Drop for PromptAnswer {
    fn drop(&mut self) {
        if let PromptAnswer::Secret { secret, .. } = self {
            secret.zeroize();
        }
    }
}

/// Non-data notifications for one terminal tab.
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionNotice {
    #[serde(rename_all = "camelCase")]
    HostKey {
        prompt_id: Uuid,
        info: HostKeyInfo,
    },
    #[serde(rename_all = "camelCase")]
    Secret {
        prompt_id: Uuid,
        prompt: SecretPrompt,
    },
    Banner {
        text: String,
    },
    Info {
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Exit {
        code: Option<u32>,
        message: Option<String>,
    },
}

#[derive(Default)]
pub struct PendingPrompts {
    waiting: Mutex<HashMap<Uuid, oneshot::Sender<PromptAnswer>>>,
}

impl PendingPrompts {
    pub fn answer(&self, id: Uuid, answer: PromptAnswer) -> bool {
        match self.waiting.lock().unwrap().remove(&id) {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }

    async fn ask(
        &self,
        notices: &Channel<SessionNotice>,
        make: impl FnOnce(Uuid) -> SessionNotice,
    ) -> Option<PromptAnswer> {
        let id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().unwrap().insert(id, tx);
        if notices.send(make(id)).is_err() {
            self.waiting.lock().unwrap().remove(&id);
            return None;
        }
        let answer = tokio::time::timeout(PROMPT_TIMEOUT, rx).await.ok().and_then(Result::ok);
        self.waiting.lock().unwrap().remove(&id);
        answer
    }
}

pub struct UiInteraction {
    pub prompts: Arc<PendingPrompts>,
    pub notices: Channel<SessionNotice>,
}

impl Interaction for UiInteraction {
    fn confirm_host_key(&self, info: HostKeyInfo) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let answer = self
                .prompts
                .ask(&self.notices, |prompt_id| SessionNotice::HostKey { prompt_id, info })
                .await;
            matches!(answer, Some(PromptAnswer::HostKey { accept: true }))
        })
    }

    fn ask_secret(&self, prompt: SecretPrompt) -> BoxFuture<'_, Option<SecretAnswer>> {
        Box::pin(async move {
            let answer = self
                .prompts
                .ask(&self.notices, |prompt_id| SessionNotice::Secret { prompt_id, prompt })
                .await;
            let mut answer = answer?;
            // `PromptAnswer` zeroizes on drop, so the secret is moved out
            // rather than copied.
            if let PromptAnswer::Secret { secret, remember } = &mut answer {
                Some(SecretAnswer {
                    secret: Zeroizing::new(std::mem::take(secret)),
                    remember: *remember,
                })
            } else {
                None
            }
        })
    }

    fn banner(&self, text: String) {
        let _ = self.notices.send(SessionNotice::Banner { text });
    }

    fn info(&self, text: String) {
        let _ = self.notices.send(SessionNotice::Info { text });
    }
}
