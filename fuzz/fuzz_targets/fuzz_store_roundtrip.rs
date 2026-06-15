// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the daemon's encrypted store/read round-trip.
//!
//! Drives `salusd::fuzz::store_roundtrip`, which seals fuzzer-controlled bytes
//! under a fuzzer-controlled key (AES-256-GCM with the key name bound as
//! additional authenticated data), persists them to an in-memory `redb`, then
//! reads and authenticates them back. This exercises the nonce handling, the
//! AAD binding, and the `redb` value (de)serialization on arbitrary input.
//!
//! The first input byte selects how many of the remaining bytes form the key;
//! the rest is the value.
//!
//! Invariants verified:
//! - No panic regardless of input.
//! - A stored value always reads back byte-for-byte identical.

#![no_main]

use libfuzzer_sys::fuzz_target;
use salusd::fuzz::store_roundtrip;

fuzz_target!(|data: &[u8]| {
    let Some((key_len, rest)) = data.split_first() else {
        return;
    };
    let split = (*key_len as usize).min(rest.len());
    let (key_bytes, value) = rest.split_at(split);
    let key = String::from_utf8_lossy(key_bytes);

    if let Ok(Some(plaintext)) = store_roundtrip(&key, value) {
        assert_eq!(plaintext, value, "store/read round-trip must preserve the value");
    }
});
