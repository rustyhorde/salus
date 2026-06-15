// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzzing facade for the daemon's storage and crypto paths.
//!
//! `ShareStore` and its methods are crate-private, so this module exposes a
//! small `pub` surface — gated behind the `fuzzing` feature — that the
//! workspace fuzz crate drives. Each helper builds (and, where useful, caches)
//! an in-memory, already-unlocked store so a fuzz target can hammer the
//! AES-256-GCM seal/open round-trip and the `find` regex path without standing
//! up a daemon, a socket, or an on-disk database.

use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use libsalus::Response;
use redb::{Database, backends::InMemoryBackend};

use crate::store::ShareStore;

/// Keys seeded into the cached store so `find` has something to match against.
const SEED_KEYS: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "db/prod/password",
    "api_key_v2",
    "service.token",
];

/// Build an in-memory `ShareStore` that has been initialized (shares generated
/// and the `CHECK_KEY` sentinel written) but is **not** yet unlocked.
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_initialized_store() -> Result<(ShareStore, Vec<String>)> {
    let db = Database::builder().create_with_backend(InMemoryBackend::new())?;
    let mut store = ShareStore::builder().redb(Arc::new(Mutex::new(db))).build();
    let shares = match store.gen_shares()? {
        Response::Shares(shares) => shares.shares().to_vec(),
        other => bail!("expected shares from gen_shares, got {other:?}"),
    };
    Ok((store, shares))
}

/// Build an in-memory `ShareStore` and unlock it with the default threshold of
/// three shares, seeding a handful of keys for the `find` path.
#[cfg_attr(coverage_nightly, coverage(off))]
fn build_unlocked_store() -> Result<ShareStore> {
    let (mut store, shares) = build_initialized_store()?;
    for share in shares.iter().take(3) {
        store.add_share(share.clone());
    }
    match store.unlock()? {
        Response::Success => {}
        other => bail!("expected successful unlock, got {other:?}"),
    }
    for key in SEED_KEYS {
        let _stored = store.store(key, format!("value-for-{key}").into_bytes())?;
    }
    Ok(store)
}

thread_local! {
    static STORE: ShareStore =
        build_unlocked_store().expect("fuzz store initialization must succeed");
}

/// Seal `value` under `key`, read it back, and return the decrypted plaintext.
///
/// Exercises the full `store` → `read` path: AES-256-GCM sealing with the key
/// name bound as additional authenticated data, the `redb` write/read, and the
/// authenticated open. A fuzz target should assert the result round-trips back
/// to `value`.
///
/// # Errors
///
/// Returns an error if the underlying seal, database, or open operation fails.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn store_roundtrip(key: &str, value: &[u8]) -> Result<Option<Vec<u8>>> {
    STORE.with(|store| {
        let _stored = store.store(key, value.to_vec())?;
        match store.read(key)? {
            Response::Value(plaintext) => Ok(plaintext),
            other => bail!("expected a value from read, got {other:?}"),
        }
    })
}

/// Compile `pattern` and match it against the seeded store keys.
///
/// Drives `ShareStore::find`, whose only attacker-controlled input is the regex
/// pattern. An invalid pattern is reported as an `Err` (never a panic).
///
/// # Errors
///
/// Returns an error if `pattern` is not a valid regular expression or the
/// database iteration fails.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn find_regex(pattern: &str) -> Result<Vec<String>> {
    STORE.with(|store| match store.find(pattern)? {
        Response::Matches(matches) => Ok(matches),
        other => bail!("expected matches from find, got {other:?}"),
    })
}
