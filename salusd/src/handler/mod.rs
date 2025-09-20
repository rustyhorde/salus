// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::sync::{Arc, Mutex};

use anyhow::{Error, Result};
use bincode::{config::standard, encode_to_vec};
use bon::Builder;
use interprocess::local_socket::traits::tokio::SendHalf;
use libsalus::{Action, Init, Response, Store};
use tokio::{
    io::AsyncWriteExt,
    spawn,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::store::ShareStore;

#[derive(Builder)]
pub(crate) struct ActionHandler<T>
where
    T: SendHalf + Unpin,
{
    sender: T,
    store: Arc<Mutex<ShareStore>>,
    #[builder(into, default = 20u64)]
    key_timeout: u64,
}

impl<T> ActionHandler<T>
where
    T: SendHalf + Unpin,
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
            Action::Unlock => self.unlock().await?,
            Action::Store(store) => self.store(store).await?,
            Action::Read(key) => self.read(key).await?,
            Action::GetThreshold => self.get_threshold().await?,
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

    async fn unlock(&mut self) -> Result<()> {
        let store_c = self.store.clone();
        let key_timeout = self.key_timeout;
        match self.unlock_store(|store| -> Result<Response> {
            let res = store.unlock();

            if res.is_ok() {
                // If we successfully unlocked the key, set a timer to clear it from memory after `key_timeout` seconds.
                // This is a basic security measure to limit the time the key is in memory.
                let interval = sleep(Duration::from_secs(key_timeout));
                let store_c = store_c.clone();
                let _blah = spawn(async move {
                    interval.await;
                    warn!("Clearing unlocked key from memory");
                    store_c.lock().unwrap().clear_key();
                });
            }
            res
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

    async fn response(&mut self, message: Response) -> Result<()> {
        let message = encode_to_vec(message, standard())?;
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
