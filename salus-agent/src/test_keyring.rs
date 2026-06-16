// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! A process-local, in-memory keyring backend for tests.
//!
//! The stock `keyring::mock` builds a fresh, non-persistent credential for every
//! `Entry::new`, but `keystore` opens a new `Entry` per operation, so the mock
//! cannot round-trip a write through a later read. This backend keys credentials
//! by `(service, account)` in a single shared map, so repeated `Entry::new` calls
//! for the same account see the same data — just like a real keyring.
//!
//! [`guard`] installs the backend once, clears it, and returns a held lock that
//! serializes keyring-touching tests so they cannot collide on the shared map
//! even when the test binary runs them on multiple threads.

use std::{
    any::Any,
    collections::HashMap,
    sync::{Mutex, MutexGuard, OnceLock},
};

use keyring::{
    Error as KeyringError, Result as KeyringResult,
    credential::{Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence},
    set_default_credential_builder,
};

type Store = Mutex<HashMap<(String, String), Vec<u8>>>;

fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn serial() -> &'static Mutex<()> {
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| Mutex::new(()))
}

fn lock_store() -> MutexGuard<'static, HashMap<(String, String), Vec<u8>>> {
    store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug)]
struct MemCredential {
    key: (String, String),
}

impl CredentialApi for MemCredential {
    fn set_secret(&self, secret: &[u8]) -> KeyringResult<()> {
        let _old = lock_store().insert(self.key.clone(), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> KeyringResult<Vec<u8>> {
        lock_store()
            .get(&self.key)
            .cloned()
            .ok_or(KeyringError::NoEntry)
    }

    fn delete_credential(&self) -> KeyringResult<()> {
        if lock_store().remove(&self.key).is_some() {
            Ok(())
        } else {
            Err(KeyringError::NoEntry)
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct MemBuilder;

impl CredentialBuilderApi for MemBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> KeyringResult<Box<Credential>> {
        Ok(Box::new(MemCredential {
            key: (service.to_string(), user.to_string()),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::ProcessOnly
    }
}

/// Install the in-memory keyring (once), clear its contents, and serialize
/// keyring-touching tests for the lifetime of the returned guard.
pub(crate) fn guard() -> MutexGuard<'static, ()> {
    static INIT: OnceLock<()> = OnceLock::new();
    let lock = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _init = INIT.get_or_init(|| set_default_credential_builder(Box::new(MemBuilder)));
    lock_store().clear();
    lock
}
