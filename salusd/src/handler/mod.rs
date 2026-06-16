// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::sync::{Arc, Mutex};

use anyhow::{Error, Result};
use bon::Builder;
use libsalus::{Action, Init, MAX_UNLOCK_SECONDS, Response, Store, UnlockTimeout, encode};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    spawn,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::store::ShareStore;

#[derive(Builder)]
pub(crate) struct ActionHandler<T>
where
    T: AsyncWrite + Unpin,
{
    sender: T,
    store: Arc<Mutex<ShareStore>>,
    #[builder(into, default = 20u64)]
    key_timeout: u64,
}

impl<T> ActionHandler<T>
where
    T: AsyncWrite + Unpin,
{
    pub(crate) async fn action_handler(&mut self, message: Action) -> Result<()> {
        match message {
            Action::GenShares(num_shares, threshold) => {
                let init = Init::builder()
                    .num_shares(num_shares)
                    .threshold(threshold)
                    .build();
                match self.initialize(init).await {
                    Ok(()) => self.gen_key().await?,
                    Err(e) => self.error(e).await?,
                }
            }
            Action::Share(share) => self.add_share(share.share()).await?,
            Action::Unlock(timeout) => self.unlock(timeout).await?,
            Action::Lock => self.lock().await?,
            Action::Store(store) => self.store(store).await?,
            Action::Read(key) => self.read(key).await?,
            Action::GetThreshold => self.get_threshold().await?,
            Action::FindKey(key) => self.find(key).await?,
        }
        Ok(())
    }

    async fn initialize(&mut self, init: Init) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> { store.initialize(init) }) {
            Ok(_response) => {
                // self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn gen_key(&mut self) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> { store.gen_shares() }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn add_share(&mut self, share: &str) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> {
            store.add_share(share);
            Ok(Response::Success)
        }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn get_threshold(&mut self) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> {
            let threshold = store.get_threshold();
            Ok(Response::Threshold(threshold))
        }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn unlock(&mut self, timeout: UnlockTimeout) -> Result<()> {
        let store_c = self.store.clone();
        // Resolve how long the key should live: the configured default, an
        // explicit duration (clamped to 24 h so a client cannot ask for more),
        // or forever (no timer at all).
        let hold_secs = match timeout {
            UnlockTimeout::Default => Some(self.key_timeout),
            UnlockTimeout::Seconds(secs) => Some(secs.min(MAX_UNLOCK_SECONDS)),
            UnlockTimeout::Forever => None,
        };
        match self.unlock_store(|store| -> Result<Response> {
            let response = store.unlock()?;

            if matches!(response, Response::Success) {
                if let Some(hold_secs) = hold_secs {
                    // We successfully unlocked the key, so set a timer to clear it
                    // from memory after `hold_secs` seconds. The timer captures the
                    // current unlock generation; a later unlock or lock bumps the
                    // generation, so this timer firing becomes a no-op and cannot
                    // clear a fresher key.
                    let generation = store.key_generation();
                    let interval = sleep(Duration::from_secs(hold_secs));
                    let store_c = store_c.clone();
                    let _blah = spawn(async move {
                        interval.await;
                        warn!("Clearing unlocked key from memory");
                        let mut store = match store_c.lock() {
                            Ok(store) => store,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        store.clear_key_if_generation(generation);
                    });
                } else {
                    warn!("Key unlocked with no auto-clear timer (forever)");
                }
            }
            Ok(response)
        }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn lock(&mut self) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> {
            store.lock();
            warn!("Store locked; unlocked key cleared from memory");
            Ok(Response::Success)
        }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn store(&mut self, value: Store) -> Result<()> {
        let (key, value) = value.into_parts();
        match self.unlock_store(|store| -> Result<Response> {
            store.store(&key, value.as_bytes().to_vec())
        }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn read(&mut self, key: String) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> { store.read(&key) }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn find(&mut self, regex: String) -> Result<()> {
        match self.unlock_store(|store| -> Result<Response> { store.find(&regex) }) {
            Ok(response) => {
                self.response(response).await?;
            }
            Err(e) => {
                self.error(e).await?;
            }
        }
        Ok(())
    }

    async fn response(&mut self, message: Response) -> Result<()> {
        let message = encode(message)?;
        self.sender.write_all(&message).await?;
        self.sender.flush().await?;
        Ok(())
    }

    async fn error(&mut self, err: Error) -> Result<()> {
        self.response(Response::Error(err.to_string())).await
    }

    fn unlock_store(
        &mut self,
        mut store_fn: impl FnMut(&mut ShareStore) -> Result<Response>,
    ) -> Result<Response> {
        let mut store = match self.store.lock() {
            Ok(share_store) => share_store,
            Err(poisoned) => poisoned.into_inner(),
        };
        store_fn(&mut store)
    }
}

#[cfg(test)]
mod test {
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use libsalus::{Action, Response, Store, decode};
    use redb::Database;

    use super::ActionHandler;
    use crate::store::ShareStore;

    fn temp_store() -> Arc<Mutex<ShareStore>> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        Arc::new(Mutex::new(
            ShareStore::builder().redb(Arc::new(Mutex::new(db))).build(),
        ))
    }

    fn handler(store: Arc<Mutex<ShareStore>>) -> ActionHandler<Vec<u8>> {
        ActionHandler::builder()
            .sender(Vec::<u8>::new())
            .store(store)
            .key_timeout(0u64)
            .build()
    }

    async fn run(action: Action) -> Result<Response> {
        let mut handler = handler(temp_store());
        handler.action_handler(action).await?;
        decode::<Response>(&handler.sender)
    }

    #[tokio::test]
    async fn gen_shares_responds_with_shares() -> Result<()> {
        match run(Action::GenShares(5, 3)).await? {
            Response::Shares(shares) => assert_eq!(shares.shares().len(), 5),
            other => panic!("expected shares, got {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn get_threshold_responds_with_default() -> Result<()> {
        assert!(matches!(
            run(Action::GetThreshold).await?,
            Response::Threshold(3)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn read_before_unlock_errors() -> Result<()> {
        assert!(matches!(
            run(Action::Read("k".to_string())).await?,
            Response::Error(_)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn store_before_unlock_errors() -> Result<()> {
        let store = Store::builder().key("k").value("v").build();
        assert!(matches!(
            run(Action::Store(store)).await?,
            Response::Error(_)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn lock_responds_success() -> Result<()> {
        assert!(matches!(run(Action::Lock).await?, Response::Success));
        Ok(())
    }
}
