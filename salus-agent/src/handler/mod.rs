// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use bon::Builder;
use libsalus::{AgentAction, AgentResponse, encode};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    spawn,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::store::{AgentState, UnsealResult};

#[derive(Builder)]
pub(crate) struct AgentHandler<T>
where
    T: AsyncWrite + Unpin,
{
    sender: T,
    store: Arc<Mutex<AgentState>>,
    #[builder(default = 3600u64)]
    cache_timeout: u64,
}

impl<T> AgentHandler<T>
where
    T: AsyncWrite + Unpin,
{
    pub(crate) async fn handle(&mut self, message: AgentAction) -> Result<()> {
        let response = match message {
            AgentAction::Status => AgentResponse::Status {
                sets: self.with_store(|store| store.status()),
            },
            AgentAction::GetAutoShares { set } => self.get_auto_shares(&set),
            AgentAction::UnsealFinal { set, passphrase } => self.unseal(&set, &passphrase),
            AgentAction::Lock { set } => {
                self.with_store(|store| store.lock(set.as_deref()));
                AgentResponse::Status {
                    sets: self.with_store(|store| store.status()),
                }
            }
            AgentAction::Reload => match self.with_store(AgentState::reload) {
                Ok(()) => AgentResponse::Status {
                    sets: self.with_store(|store| store.status()),
                },
                Err(e) => AgentResponse::Error(e.to_string()),
            },
        };
        self.respond(response).await
    }

    fn get_auto_shares(&self, set: &str) -> AgentResponse {
        let (shares, empty) = self.with_store(|store| (store.auto_shares(set), store.is_empty()));
        match shares {
            Some(shares) => AgentResponse::AutoShares(shares),
            None if empty => AgentResponse::Unenrolled,
            None => AgentResponse::UnknownSet,
        }
    }

    fn unseal(&self, set: &str, passphrase: &str) -> AgentResponse {
        let cache_timeout = self.cache_timeout;
        let store_c = self.store.clone();
        let result = self.with_store(|store| store.unseal(set, passphrase, cache_timeout));
        match result {
            Ok(UnsealResult::Unknown) => {
                if self.with_store(|store| store.is_empty()) {
                    AgentResponse::Unenrolled
                } else {
                    AgentResponse::UnknownSet
                }
            }
            Ok(UnsealResult::Bad) => AgentResponse::BadPassphrase,
            Ok(UnsealResult::Share { value, arm_timer }) => {
                if let Some(generation) = arm_timer {
                    // The share was freshly cached; arm a timer to clear it after
                    // `cache_timeout` seconds. The generation guard means a later
                    // unseal or lock makes this timer a no-op.
                    let set = set.to_string();
                    let interval = sleep(Duration::from_secs(cache_timeout));
                    let _timer = spawn(async move {
                        interval.await;
                        warn!("Clearing cached final share for set '{set}'");
                        let mut store = match store_c.lock() {
                            Ok(store) => store,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        store.clear_cache_if_generation(&set, generation);
                    });
                }
                AgentResponse::FinalShare(value)
            }
            Err(e) => AgentResponse::Error(e.to_string()),
        }
    }

    async fn respond(&mut self, response: AgentResponse) -> Result<()> {
        let message = encode(response)?;
        self.sender.write_all(&message).await?;
        self.sender.flush().await?;
        Ok(())
    }

    fn with_store<R>(&self, store_fn: impl FnOnce(&mut AgentState) -> R) -> R {
        let mut store = match self.store.lock() {
            Ok(store) => store,
            Err(poisoned) => poisoned.into_inner(),
        };
        store_fn(&mut store)
    }
}

#[cfg(test)]
mod test {
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use libsalus::{AgentAction, AgentResponse, decode};

    use super::AgentHandler;
    use crate::{keystore, store::AgentState, test_keyring::guard};

    fn handler(store: Arc<Mutex<AgentState>>, cache_timeout: u64) -> AgentHandler<Vec<u8>> {
        AgentHandler::builder()
            .sender(Vec::<u8>::new())
            .store(store)
            .cache_timeout(cache_timeout)
            .build()
    }

    async fn run(
        state: AgentState,
        cache_timeout: u64,
        action: AgentAction,
    ) -> Result<AgentResponse> {
        let mut handler = handler(Arc::new(Mutex::new(state)), cache_timeout);
        handler.handle(action).await?;
        decode::<AgentResponse>(&handler.sender)
    }

    fn enroll_alpha() -> Result<()> {
        keystore::enroll_full(
            "alpha",
            &["s0".into(), "s1".into(), "final".into()],
            "pass",
            false,
            false,
        )
    }

    #[tokio::test]
    async fn status_responds_with_empty_sets() -> Result<()> {
        let resp = run(AgentState::default(), 0, AgentAction::Status).await?;
        assert!(matches!(resp, AgentResponse::Status { sets } if sets.is_empty()));
        Ok(())
    }

    #[tokio::test]
    async fn get_auto_shares_on_empty_is_unenrolled() -> Result<()> {
        let resp = run(
            AgentState::default(),
            0,
            AgentAction::GetAutoShares {
                set: "alpha".into(),
            },
        )
        .await?;
        assert!(matches!(resp, AgentResponse::Unenrolled));
        Ok(())
    }

    #[tokio::test]
    async fn get_auto_shares_known_and_unknown() -> Result<()> {
        // Touch the keyring only while building the states; drop the guard before
        // any `.await` so a `MutexGuard` is never held across an await point.
        let (known, unknown) = {
            let _g = guard();
            enroll_alpha()?;
            (AgentState::load()?, AgentState::load()?)
        };
        let resp = run(
            known,
            0,
            AgentAction::GetAutoShares {
                set: "alpha".into(),
            },
        )
        .await?;
        assert!(matches!(resp, AgentResponse::AutoShares(shares) if shares.len() == 2));

        let resp = run(
            unknown,
            0,
            AgentAction::GetAutoShares {
                set: "missing".into(),
            },
        )
        .await?;
        assert!(matches!(resp, AgentResponse::UnknownSet));
        Ok(())
    }

    #[tokio::test]
    async fn unseal_bad_passphrase_and_success() -> Result<()> {
        let (bad, good) = {
            let _g = guard();
            enroll_alpha()?;
            (AgentState::load()?, AgentState::load()?)
        };
        let resp = run(
            bad,
            0,
            AgentAction::UnsealFinal {
                set: "alpha".into(),
                passphrase: "wrong".into(),
            },
        )
        .await?;
        assert!(matches!(resp, AgentResponse::BadPassphrase));

        // cache_timeout = 0 so no clear timer is spawned.
        let resp = run(
            good,
            0,
            AgentAction::UnsealFinal {
                set: "alpha".into(),
                passphrase: "pass".into(),
            },
        )
        .await?;
        assert!(matches!(resp, AgentResponse::FinalShare(share) if share == "final"));
        Ok(())
    }

    #[tokio::test]
    async fn lock_responds_with_status() -> Result<()> {
        let resp = run(AgentState::default(), 0, AgentAction::Lock { set: None }).await?;
        assert!(matches!(resp, AgentResponse::Status { .. }));
        Ok(())
    }
}
