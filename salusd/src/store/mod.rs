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
use libsalus::{Init, Response, Shares, SsssConfig, fuzzy_rank, gen_shares, unlock_key};
use redb::{Database, ReadableDatabase, ReadableTable};
use regex::Regex;
use tracing::{error, info, trace};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    db::{
        CHECK_KEY_KEY, INITIALIZED_KEY, NUM_SHARES_KEY, SALUS_CONFIG_TABLE_DEF,
        SALUS_VAL_TABLE_DEF, THRESHOLD_KEY, delete_value, read_value, unlock_redb,
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
    key: Option<Zeroizing<Vec<u8>>>,
    redb: Arc<Mutex<Database>>,
    /// Incremented on every successful unlock so that stale key-clear timers
    /// (from earlier unlocks) become no-ops. See `clear_key_if_generation`.
    #[builder(default)]
    key_generation: u64,
}

impl ShareStore {
    fn clear_shares(&mut self) {
        self.shares.zeroize();
    }

    pub(crate) fn clear_key(&mut self) {
        self.key = None;
    }

    /// Clear the unlocked key only if the store has not been unlocked again
    /// since the timer that calls this was started.
    pub(crate) fn clear_key_if_generation(&mut self, generation: u64) {
        if self.key_generation == generation {
            self.clear_key();
        }
    }

    /// The current unlock generation, used to invalidate stale clear timers.
    pub(crate) fn key_generation(&self) -> u64 {
        self.key_generation
    }

    /// Force-clear the unlocked key immediately and bump the unlock generation
    /// so any pending auto-clear timer becomes a no-op.
    pub(crate) fn lock(&mut self) {
        self.clear_key();
        self.key_generation = self.key_generation.wrapping_add(1);
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
            let mut key = Zeroizing::new([0u8; 32]);
            rand::fill(&mut *key)?;
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
                    let rnkey = RandomizedNonceKey::new(&AES_256_GCM, key.as_slice())
                        .with_context(|| Error::NonceKeyGen)?;
                    let mut check_key = CHECK_KEY_KEY.as_bytes().to_vec();
                    let nonce = rnkey.seal_in_place_append_tag(
                        Aad::from(CHECK_KEY_KEY.as_bytes()),
                        &mut check_key,
                    )?;
                    unlock_redb(&self.redb, |db| -> Result<()> {
                        let salus_val = SalusVal::from_parts(*nonce.as_ref(), &check_key);
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
        let mut unlocked = false;
        match unlock_key(&self.shares) {
            Ok(key) => {
                unlock_redb(&self.redb, |redb_c| -> Result<()> {
                    match read_value::<String, SalusVal>(
                        redb_c,
                        SALUS_VAL_TABLE_DEF,
                        CHECK_KEY_KEY.to_string(),
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
                            let nonce = Nonce::from(&sv.nonce()?);
                            let rnkey = RandomizedNonceKey::new(&AES_256_GCM, &key)
                                .with_context(|| Error::NonceKeyGen)?;
                            let mut ciphertext = sv.ciphertext()?.to_vec();
                            // A failed open here means the reconstructed key (and
                            // therefore the supplied shares) is wrong. That is a
                            // normal unlock failure, not a panic and not a hard error.
                            match rnkey.open_in_place(
                                nonce,
                                Aad::from(CHECK_KEY_KEY.as_bytes()),
                                &mut ciphertext,
                            ) {
                                Ok(plaintext_b) if plaintext_b == CHECK_KEY_KEY.as_bytes() => {
                                    info!("Key successfully unlocked and verified.");
                                    self.key = Some(key.clone());
                                    unlocked = true;
                                }
                                Ok(_) | Err(_) => {
                                    error!("Failed to unlock key with provided shares");
                                }
                            }
                        }
                    }
                    Ok(())
                })?;
            }
            Err(e) => error!("Failed to reconstruct key from provided shares: {e}"),
        }
        self.clear_shares();
        if unlocked {
            self.key_generation = self.key_generation.wrapping_add(1);
            Ok(Response::Success)
        } else {
            Ok(Response::UnlockFailed)
        }
    }

    pub(crate) fn store(&self, key: &str, mut value: Vec<u8>, force: bool) -> Result<Response> {
        if let Some(enc_key) = &self.key {
            // Collision protection: unless the caller forces the write, refuse to
            // overwrite an existing key. Checked before sealing so a refused
            // overwrite does no needless encryption.
            if !force {
                let mut exists = false;
                unlock_redb(&self.redb, |db| -> Result<()> {
                    exists =
                        read_value::<String, SalusVal>(db, SALUS_VAL_TABLE_DEF, key.to_string())?
                            .is_some();
                    Ok(())
                })?;
                if exists {
                    info!("Refusing to overwrite existing key without force: {key}");
                    return Ok(Response::KeyExists);
                }
            }
            let rnkey = RandomizedNonceKey::new(&AES_256_GCM, enc_key)
                .with_context(|| Error::NonceKeyGen)?;
            let nonce = rnkey.seal_in_place_append_tag(Aad::from(key.as_bytes()), &mut value)?;
            unlock_redb(&self.redb, |db| -> Result<()> {
                let salus_val = SalusVal::from_parts(*nonce.as_ref(), &value);
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

    pub(crate) fn read(&self, key: &str) -> Result<Response> {
        if let Some(enc_key) = &self.key {
            let mut response = Response::KeyNotFound;
            unlock_redb(&self.redb, |db| -> Result<()> {
                match read_value::<String, SalusVal>(db, SALUS_VAL_TABLE_DEF, key.to_string()) {
                    Err(e) => {
                        error!("Error reading value from database: {e}");
                        return Err(e);
                    }
                    Ok(None) => {
                        info!("Key not found: {key}");
                        response = Response::Value(None);
                    }
                    Ok(Some(svag)) => {
                        let sv = svag.value();
                        let nonce = Nonce::from(&sv.nonce()?);
                        let rnkey = RandomizedNonceKey::new(&AES_256_GCM, enc_key)
                            .with_context(|| Error::NonceKeyGen)?;
                        let mut ciphertext = sv.ciphertext()?.to_vec();
                        match rnkey.open_in_place(nonce, Aad::from(key.as_bytes()), &mut ciphertext)
                        {
                            Err(e) => {
                                error!("Error decrypting value: {e}");
                                return Err(e.into());
                            }
                            Ok(plaintext_b) => {
                                trace!("Read and decrypted value for key {key}");
                                response = Response::Value(Some(plaintext_b.to_vec()));
                            }
                        }
                    }
                }
                Ok(())
            })?;
            Ok(response)
        } else {
            Err(Error::StoreNotUnlocked.into())
        }
    }

    pub(crate) fn delete(&self, key: &str) -> Result<Response> {
        if self.key.is_none() {
            return Err(Error::StoreNotUnlocked.into());
        }
        let mut removed = false;
        unlock_redb(&self.redb, |db| -> Result<()> {
            match delete_value::<String, SalusVal>(db, SALUS_VAL_TABLE_DEF, key.to_string()) {
                Err(e) => {
                    error!("Error deleting value from database: {e}");
                    return Err(e);
                }
                Ok(existed) => {
                    removed = existed;
                    if existed {
                        info!("Deleted value under key: {key}");
                    } else {
                        info!("Key not found for delete: {key}");
                    }
                }
            }
            Ok(())
        })?;
        if removed {
            Ok(Response::Success)
        } else {
            Ok(Response::KeyNotFound)
        }
    }

    pub(crate) fn find(&self, regex: &str) -> Result<Response> {
        // Key names are only revealed to an unlocked client: the less exposed
        // while locked, the better.
        if self.key.is_none() {
            return Err(Error::StoreNotUnlocked.into());
        }
        let mut matches = vec![];
        trace!("Finding keys matching regex: {regex}");
        let re = Regex::new(regex).with_context(|| Error::InvalidRegex)?;

        unlock_redb(&self.redb, |db| -> Result<()> {
            let read_txn = db.begin_read()?;
            let table = read_txn.open_table(SALUS_VAL_TABLE_DEF)?;
            for iter_res in table.iter()? {
                let (key_bytes, _) = iter_res.with_context(|| Error::TableIterRead)?;
                let key_str = key_bytes.value();
                if re.is_match(&key_str) {
                    matches.push(key_str.clone());
                }
            }
            Ok(())
        })?;
        Ok(Response::Matches(matches))
    }

    /// Predictively (fuzzy) search key names, returning ranked matches.
    ///
    /// Like [`find`](Self::find), this requires the store to be unlocked so key
    /// names are never enumerable without the key. The `CHECK_KEY` sentinel row
    /// is internal bookkeeping and is excluded from the results.
    pub(crate) fn search(&self, query: &str, limit: Option<usize>) -> Result<Response> {
        if self.key.is_none() {
            return Err(Error::StoreNotUnlocked.into());
        }
        trace!("Searching keys for query: {query}");
        let mut keys = vec![];
        unlock_redb(&self.redb, |db| -> Result<()> {
            let read_txn = db.begin_read()?;
            let table = read_txn.open_table(SALUS_VAL_TABLE_DEF)?;
            for iter_res in table.iter()? {
                let (key_bytes, _) = iter_res.with_context(|| Error::TableIterRead)?;
                let key_str = key_bytes.value();
                if key_str != CHECK_KEY_KEY {
                    keys.push(key_str.clone());
                }
            }
            Ok(())
        })?;
        Ok(Response::Matches(fuzzy_rank(query, keys, limit)))
    }
}

#[cfg(test)]
mod test {
    use std::sync::{Arc, Mutex};

    use anyhow::{Result, anyhow, bail};
    use libsalus::Response;
    use redb::Database;

    use super::ShareStore;
    use crate::db::{
        SALUS_VAL_TABLE_DEF, read_value, unlock_redb, values::salus::SalusVal, write_value,
    };

    fn temp_store() -> Result<ShareStore> {
        // Each test gets its own isolated in-memory database. This avoids the
        // filesystem entirely, so parallel tests can never collide on a shared
        // path and trigger redb's `DatabaseAlreadyOpen`.
        let db = Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;
        Ok(ShareStore::builder().redb(Arc::new(Mutex::new(db))).build())
    }

    fn gen_and_collect(store: &mut ShareStore) -> Result<Vec<String>> {
        match store.gen_shares()? {
            Response::Shares(shares) => Ok(shares.shares().to_vec()),
            other => bail!("expected shares, got {other:?}"),
        }
    }

    #[test]
    fn unlock_with_correct_shares_succeeds() -> Result<()> {
        let mut store = temp_store()?;
        let shares = gen_and_collect(&mut store)?;
        // Default threshold is 3.
        for share in shares.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::Success));
        assert!(store.key.is_some());
        assert_eq!(store.key_generation(), 1);
        Ok(())
    }

    #[test]
    fn unlock_with_wrong_shares_fails_without_panic() -> Result<()> {
        let mut store = temp_store()?;
        let _shares = gen_and_collect(&mut store)?;

        // Shares from a *different* store reconstruct a different (wrong) key, so
        // the sentinel GCM open fails. This used to `.unwrap()`-panic (H2).
        let mut other = temp_store()?;
        let wrong = gen_and_collect(&mut other)?;
        for share in wrong.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::UnlockFailed));
        assert!(store.key.is_none());
        assert_eq!(store.key_generation(), 0);
        Ok(())
    }

    #[test]
    fn delete_removes_stored_value() -> Result<()> {
        let mut store = temp_store()?;
        let shares = gen_and_collect(&mut store)?;
        for share in shares.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::Success));
        assert!(matches!(
            store.store("alpha", b"top-secret".to_vec(), false)?,
            Response::Success
        ));

        // Deleting a present key reports success and the value is gone.
        assert!(matches!(store.delete("alpha")?, Response::Success));
        assert!(matches!(store.read("alpha")?, Response::Value(None)));

        // Deleting again is idempotent: nothing to remove.
        assert!(matches!(store.delete("alpha")?, Response::KeyNotFound));
        Ok(())
    }

    #[test]
    fn store_refuses_overwrite_without_force() -> Result<()> {
        let mut store = temp_store()?;
        let shares = gen_and_collect(&mut store)?;
        for share in shares.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::Success));

        // First write under a fresh key succeeds.
        assert!(matches!(
            store.store("alpha", b"first".to_vec(), false)?,
            Response::Success
        ));

        // A second write without force is refused and leaves the old value intact.
        assert!(matches!(
            store.store("alpha", b"second".to_vec(), false)?,
            Response::KeyExists
        ));
        match store.read("alpha")? {
            Response::Value(Some(value)) => assert_eq!(value, b"first"),
            other => bail!("expected the original value, got {other:?}"),
        }

        // With force the value is overwritten.
        assert!(matches!(
            store.store("alpha", b"second".to_vec(), true)?,
            Response::Success
        ));
        match store.read("alpha")? {
            Response::Value(Some(value)) => assert_eq!(value, b"second"),
            other => bail!("expected the overwritten value, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn delete_before_unlock_errors() -> Result<()> {
        let store = temp_store()?;
        assert!(store.delete("alpha").is_err());
        Ok(())
    }

    #[test]
    fn find_and_search_before_unlock_error() -> Result<()> {
        // Key names must not be enumerable without the key.
        let store = temp_store()?;
        assert!(store.find(".*").is_err());
        assert!(store.search("", None).is_err());
        Ok(())
    }

    #[test]
    fn search_returns_ranked_matches() -> Result<()> {
        let mut store = temp_store()?;
        let shares = gen_and_collect(&mut store)?;
        for share in shares.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::Success));
        for key in ["aws-prod-key", "aws-staging", "github-token"] {
            assert!(matches!(
                store.store(key, b"v".to_vec(), false)?,
                Response::Success
            ));
        }

        // A fuzzy query returns only the relevant keys, and never the internal
        // CHECK_KEY sentinel.
        match store.search("aws", None)? {
            Response::Matches(matches) => {
                assert!(matches.iter().any(|k| k == "aws-prod-key"));
                assert!(matches.iter().any(|k| k == "aws-staging"));
                assert!(!matches.iter().any(|k| k == "github-token"));
                assert!(!matches.iter().any(|k| k == "CHECK_KEY"));
            }
            other => bail!("expected matches, got {other:?}"),
        }

        // An empty query lists every stored key (sorted), still excluding the
        // sentinel.
        match store.search("", None)? {
            Response::Matches(matches) => {
                assert_eq!(matches, vec!["aws-prod-key", "aws-staging", "github-token"]);
            }
            other => bail!("expected matches, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn find_returns_regex_matches() -> Result<()> {
        let mut store = temp_store()?;
        let shares = gen_and_collect(&mut store)?;
        for share in shares.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::Success));
        for key in ["aws-prod-key", "aws-staging", "github-token"] {
            assert!(matches!(
                store.store(key, b"v".to_vec(), false)?,
                Response::Success
            ));
        }

        // A regex returns only the matching keys, and never the internal
        // CHECK_KEY sentinel.
        match store.find("aws.*")? {
            Response::Matches(matches) => {
                assert!(matches.iter().any(|k| k == "aws-prod-key"));
                assert!(matches.iter().any(|k| k == "aws-staging"));
                assert!(!matches.iter().any(|k| k == "github-token"));
                assert!(!matches.iter().any(|k| k == "CHECK_KEY"));
            }
            other => bail!("expected matches, got {other:?}"),
        }

        // A non-matching regex returns an empty match set.
        match store.find("zzz-no-such-key")? {
            Response::Matches(matches) => assert!(matches.is_empty()),
            other => bail!("expected empty matches, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn relocated_ciphertext_fails_to_decrypt() -> Result<()> {
        let mut store = temp_store()?;
        let shares = gen_and_collect(&mut store)?;
        for share in shares.iter().take(3) {
            store.add_share(share.clone());
        }
        assert!(matches!(store.unlock()?, Response::Success));
        assert!(matches!(
            store.store("alpha", b"top-secret".to_vec(), false)?,
            Response::Success
        ));

        // Copy alpha's sealed blob verbatim under a different key name.
        unlock_redb(&store.redb, |db| {
            let sv = read_value::<String, SalusVal>(db, SALUS_VAL_TABLE_DEF, "alpha".to_string())?
                .ok_or_else(|| anyhow!("alpha present"))?
                .value();
            write_value::<String, SalusVal>(db, SALUS_VAL_TABLE_DEF, "beta".to_string(), sv)
        })?;

        // Reading under "beta" must fail: the key name is bound as AAD (H1).
        assert!(store.read("beta").is_err());
        // The original key name still decrypts.
        assert!(matches!(store.read("alpha")?, Response::Value(Some(_))));
        Ok(())
    }
}
