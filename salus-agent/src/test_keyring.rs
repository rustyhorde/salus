// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! A process-local, in-memory keyring backend for tests.
//!
//! Production code reaches the keyring through `keyring`'s `v1` API
//! ([`keyring::Entry`]). The first `Entry::new` in a process installs the
//! platform-native store (Secret Service / Keychain) as `keyring_core`'s
//! default via a private `Once`. Left to its own devices that would route the
//! tests at the *real* OS keyring — mutating (and reading stale state from) the
//! developer's actual credentials.
//!
//! [`guard`] defuses that: it fires the `Once` once with a throwaway entry, then
//! installs a fresh `keyring-core` [`mock::Store`] as the default. The mock keys
//! credentials by `(service, account)` in a per-instance map, so the new
//! `Entry` that `keystore` opens for each operation still round-trips a write
//! through a later read. Installing a brand-new mock per call clears any state a
//! prior test left behind, and the returned guard serializes keyring-touching
//! tests so they cannot collide on the process-global default store.

use std::sync::{Mutex, MutexGuard, OnceLock};

use keyring_core::{mock, set_default_store};

fn serial() -> &'static Mutex<()> {
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| Mutex::new(()))
}

/// Install a fresh in-memory keyring (clearing any prior contents) and
/// serialize keyring-touching tests for the lifetime of the returned guard.
pub fn guard() -> MutexGuard<'static, ()> {
    let lock = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Fire the v1 `Once` now (it installs the native store, or nothing if the
    // platform store is unavailable) so it cannot clobber our mock later. The
    // throwaway entry is never read or written.
    let _native = keyring::Entry::new("salus", "__test_store_init__");
    // `mock::Store::new` is infallible in practice; if it ever failed we'd
    // simply leave the previous default store in place rather than panic.
    if let Ok(store) = mock::Store::new() {
        set_default_store(store);
    }
    lock
}
