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
    aead::{AES_256_GCM, Aad, Nonce, RandomizedNonceKey},
    rand,
};
use bon::Builder;
use libsalus::{Init, Response, Shares, SsssConfig, gen_shares, unlock_key};
use redb::Database;
use tracing::{error, info, trace};

use crate::{
    db::{
        CHECK_KEY_KEY, INITIALIZED_KEY, NUM_SHARES_KEY, SALUS_CONFIG_TABLE_DEF,
        SALUS_VAL_TABLE_DEF, THRESHOLD_KEY, read_value, unlock_redb,
        values::{config::ConfigVal, salus::SalusVal},
        write_value,
    },
    error::Error,
};

#[derive(Builder)]
pub(crate) struct ShareStore {
    #[builder(default)]
    shares: Vec<String>,
    #[allow(dead_code)]
    key: Option<Vec<u8>>,
    redb: Arc<Mutex<Database>>,
}

impl ShareStore {
    #[allow(dead_code)]
    fn clear_shares(&mut self) {
        self.shares.clear();
    }

    pub(crate) fn clear_key(&mut self) {
        self.key = None;
    }

    pub(crate) fn add_share<S: Into<String>>(&mut self, share: S) {
        self.shares.push(share.into());
    }

    pub(crate) fn initialize(&mut self, init: Init) -> Result<Response> {
        trace!(
            "Initializing share store with {} shares and threshold {}",
            init.num_shares(),
            init.threshold()
        );
        unlock_redb(&self.redb, |db| -> Result<()> {
            write_value::<&str, ConfigVal>(
                db,
                SALUS_CONFIG_TABLE_DEF,
                NUM_SHARES_KEY,
                ConfigVal::from_value(init.num_shares())?,
            )?;
            write_value::<&str, ConfigVal>(
                db,
                SALUS_CONFIG_TABLE_DEF,
                THRESHOLD_KEY,
                ConfigVal::from_value(init.threshold())?,
            )?;
            Ok(())
        })?;
        Ok(Response::Success)
    }

    pub(crate) fn gen_shares(&mut self) -> Result<Response> {
        trace!("Generating shares for share store");
        let mut initialized = false;
        unlock_redb(&self.redb, |db| -> Result<()> {
            if let Ok(init_opt) =
                read_value::<&str, ConfigVal>(db, SALUS_CONFIG_TABLE_DEF, INITIALIZED_KEY)
                && let Some(init) = init_opt
            {
                initialized = init.value().to_value::<bool>()?;
            }
            Ok(())
        })?;

        if initialized {
            Ok(Response::AlreadyInitialiazed)
        } else {
            let mut key = [0u8; 32];
            rand::fill(&mut key)?;
            let mut num_shares = 5;
            let mut threshold = 3;

            unlock_redb(&self.redb, |db| -> Result<()> {
                if let Ok(num_shares_opt) =
                    read_value::<&str, ConfigVal>(db, SALUS_CONFIG_TABLE_DEF, NUM_SHARES_KEY)
                    && let Some(num_shares_ag) = num_shares_opt
                {
                    num_shares = num_shares_ag.value().to_value::<u8>()?;
                }
                if let Ok(threshold_opt) =
                    read_value::<&str, ConfigVal>(db, SALUS_CONFIG_TABLE_DEF, THRESHOLD_KEY)
                    && let Some(threshold_ag) = threshold_opt
                {
                    threshold = threshold_ag.value().to_value::<u8>()?;
                }
                Ok(())
            })?;

            trace!("Generating {num_shares} shares with threshold {threshold}");
            match gen_shares(
                &SsssConfig::builder()
                    .num_shares(num_shares)
                    .threshold(threshold)
                    .build(),
                &key,
            ) {
                Ok(shares) => {
                    let rnkey = RandomizedNonceKey::new(&AES_256_GCM, &key)
                        .with_context(|| Error::NonceKeyGen)?;
                    let mut check_key = CHECK_KEY_KEY.as_bytes().to_vec();
                    let nonce = rnkey.seal_in_place_append_tag(Aad::empty(), &mut check_key)?;
                    unlock_redb(&self.redb, |db| -> Result<()> {
                        let salus_val = SalusVal::builder()
                            .nonce(*nonce.as_ref())
                            .ciphertext(check_key.clone())
                            .build();
                        write_value::<String, SalusVal>(
                            db,
                            SALUS_VAL_TABLE_DEF,
                            CHECK_KEY_KEY.to_string(),
                            salus_val,
                        )?;
                        write_value::<&str, ConfigVal>(
                            db,
                            SALUS_CONFIG_TABLE_DEF,
                            INITIALIZED_KEY,
                            ConfigVal::from_value(true)?,
                        )?;
                        Ok(())
                    })?;
                    Ok(Response::Shares(Shares::builder().shares(shares).build()))
                }
                Err(_) => Err(Error::ShareGeneration.into()),
            }
        }
    }

    pub(crate) fn get_threshold(&self) -> u8 {
        let mut threshold = 3;
        if let Ok(()) = unlock_redb(&self.redb, |db| -> Result<()> {
            if let Ok(threshold_opt) =
                read_value::<&str, ConfigVal>(db, SALUS_CONFIG_TABLE_DEF, THRESHOLD_KEY)
                && let Some(threshold_ag) = threshold_opt
            {
                threshold = threshold_ag.value().to_value::<u8>()?;
            }
            Ok(())
        }) {}
        threshold
    }

    pub(crate) fn unlock(&mut self) -> Result<Response> {
        match unlock_key(&self.shares) {
            Ok(key) => {
                unlock_redb(&self.redb, |redb_c| -> Result<()> {
                    match read_value::<String, SalusVal>(
                        redb_c,
                        SALUS_VAL_TABLE_DEF,
                        "CHECK_KEY".to_string(),
                    ) {
                        Err(e) => {
                            error!("Error reading CHECK_KEY from database: {e}");
                            return Err(e);
                        }
                        Ok(None) => {
                            error!("CHECK_KEY not found in database");
                            return Err(Error::CheckKeyNotFound.into());
                        }
                        Ok(Some(svag)) => {
                            let sv = svag.value();
                            let nonce = Nonce::from(&sv.nonce());
                            let rnkey = RandomizedNonceKey::new(&AES_256_GCM, &key)
                                .with_context(|| Error::NonceKeyGen)
                                .unwrap();
                            let mut ciphertext = sv.ciphertext().clone();
                            let plaintext_b = rnkey
                                .open_in_place(nonce, Aad::empty(), &mut ciphertext)
                                .with_context(|| Error::NonceKeyGen)
                                .unwrap();
                            let plaintext = String::from_utf8_lossy(plaintext_b).to_string();
                            trace!("Unlocked key with shares, got plaintext: {plaintext}");
                            if plaintext == "CHECK_KEY" {
                                info!("Key successfully unlocked and verified.");
                                self.key = Some(key.clone());
                            } else {
                                error!("Failed to unlock key with provided shares");
                            }
                        }
                    }
                    Ok(())
                })?;
            }
            Err(e) => error!("Failed to unlock key with provided shares: {e}"),
        }
        self.clear_shares();
        Ok(Response::Success)
    }

    pub(crate) fn store(&self, key: &str, mut value: Vec<u8>) -> Result<Response> {
        if let Some(enc_key) = &self.key {
            let rnkey = RandomizedNonceKey::new(&AES_256_GCM, enc_key)
                .with_context(|| Error::NonceKeyGen)?;
            let nonce = rnkey.seal_in_place_append_tag(Aad::empty(), &mut value)?;
            unlock_redb(&self.redb, |db| -> Result<()> {
                let salus_val = SalusVal::builder()
                    .nonce(*nonce.as_ref())
                    .ciphertext(value.clone())
                    .build();
                match write_value::<String, SalusVal>(
                    db,
                    SALUS_VAL_TABLE_DEF,
                    key.to_string(),
                    salus_val,
                ) {
                    Err(e) => {
                        error!("Error writing value to database: {e}");
                        return Err(e);
                    }
                    Ok(()) => {
                        info!("Stored value under key: {key}");
                    }
                }
                Ok(())
            })?;
            Ok(Response::Success)
        } else {
            Err(Error::StoreNotUnlocked.into())
        }
    }
}
