// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use aws_lc_rs::{
    aead::{AES_256_GCM, Aad, RandomizedNonceKey},
    rand,
};
use bincode::{config::standard, encode_to_vec};
use interprocess::local_socket::traits::tokio::SendHalf;
use libsalus::{Action, Response, Shares, SsssConfig, Store, gen_shares};
use redb::Database;
use tokio::{io::AsyncWriteExt, sync::mpsc::UnboundedSender};
use tracing::error;

use crate::{
    db::{
        SALUS_CONFIG_TABLE_DEF, SALUS_VAL_TABLE_DEF, SalusVal, read_value, unlock_redb, write_value,
    },
    error::Error,
};

pub(crate) enum ShareStoreMessage {
    AddShare(String),
    Unlock,
    ClearKey,
    Init,
    Store(Store),
}

#[derive(Default)]
pub(crate) struct ShareStore {
    shares: Vec<String>,
    key: Option<Vec<u8>>,
}

impl ShareStore {
    pub(crate) fn clear_shares(&mut self) {
        self.shares.clear();
    }

    pub(crate) fn clear_key(&mut self) {
        self.key = None;
    }

    pub(crate) fn add_key(&mut self, key: Vec<u8>) {
        self.key = Some(key);
    }

    pub(crate) fn add_share(&mut self, share: String) {
        self.shares.push(share);
    }

    pub(crate) fn shares(&self) -> Vec<String> {
        self.shares.clone()
    }

    pub(crate) fn key(&self) -> Option<Vec<u8>> {
        self.key.clone()
    }
}

pub(crate) async fn handler<T: SendHalf + Unpin>(
    sender: &mut T,
    message: Action,
    stx: UnboundedSender<ShareStoreMessage>,
    redb: Arc<Mutex<Database>>,
) -> Result<()> {
    match message {
        Action::Genkey => {
            let mut initialized = false;
            unlock_redb(&redb, |db| -> Result<()> {
                if let Ok(init_opt) =
                    read_value::<&str, bool>(db, SALUS_CONFIG_TABLE_DEF, "INITIALIZED")
                    && let Some(init) = init_opt
                {
                    initialized = init.value();
                }
                Ok(())
            })?;

            if initialized {
                response(sender, Response::AlreadyInitialiazed).await?;
            } else {
                let mut key = [0u8; 32];
                rand::fill(&mut key)?;
                if let Ok(shares) = gen_shares(&SsssConfig::default(), &key) {
                    let key = RandomizedNonceKey::new(&AES_256_GCM, &key)
                        .with_context(|| Error::NonceKeyGen)?;
                    let mut check_key = b"CHECK_KEY".to_vec();
                    let nonce = key.seal_in_place_append_tag(Aad::empty(), &mut check_key)?;
                    unlock_redb(&redb, |db| -> Result<()> {
                        let salus_val = SalusVal::builder()
                            .nonce(*nonce.as_ref())
                            .ciphertext(check_key.clone())
                            .build();
                        if let Err(e) = write_value::<String, SalusVal>(
                            db,
                            SALUS_VAL_TABLE_DEF,
                            "CHECK_KEY".to_string(),
                            salus_val,
                        ) {
                            error!("Error writing CHECK_KEY to database: {e}");
                            return Err(e);
                        }
                        if let Err(e) = write_value::<&str, bool>(
                            db,
                            SALUS_CONFIG_TABLE_DEF,
                            "INITIALIZED",
                            true,
                        ) {
                            error!("Error writing INITIALIZED to database: {e}");
                            return Err(e);
                        }
                        Ok(())
                    })?;
                    let shares_msg = Response::Shares(Shares::builder().shares(shares).build());
                    response(sender, shares_msg).await?;
                } else {
                    error!("Error generating shares");
                    error(sender).await?;
                }
                unlock_redb(&redb, |db| -> Result<()> {
                    write_value::<&str, bool>(db, SALUS_CONFIG_TABLE_DEF, "INITIALIZED", true)
                })?;
            }
        }
        Action::Share(share) => {
            stx.send(ShareStoreMessage::AddShare(share.share().to_string()))?;
            success(sender).await?;
        }
        Action::Unlock => {
            stx.send(ShareStoreMessage::Unlock)?;
            success(sender).await?;
        }
        Action::Init(_init) => {
            stx.send(ShareStoreMessage::Init)?;
            success(sender).await?;
        }
        Action::Store(store) => {
            stx.send(ShareStoreMessage::Store(store))?;
            success(sender).await?;
        }
    }
    Ok(())
}

async fn response<T: SendHalf + Unpin>(sender: &mut T, message: Response) -> Result<()> {
    let message = encode_to_vec(message, standard())?;
    sender.write_all(&message).await?;
    sender.flush().await?;
    Ok(())
}

async fn success<T: SendHalf + Unpin>(sender: &mut T) -> Result<()> {
    response(sender, Response::Success).await
}

async fn error<T: SendHalf + Unpin>(sender: &mut T) -> Result<()> {
    response(sender, Response::Error).await
}
