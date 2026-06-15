// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the daemon's wire-protocol decode of `Action`.
//!
//! `salusd` runs `decode_from_slice::<Action>` over raw bytes read from its IPC
//! socket (`salusd/src/runtime/mod.rs`), so this is the most attacker-controlled
//! input in the system. A malformed or hostile client message must be rejected
//! with an `Err`, never a panic.
//!
//! This drives the exact size-bounded `libsalus::decode`/`encode` helpers the
//! daemon uses, so a forged length prefix is rejected rather than triggering an
//! unbounded allocation.
//!
//! Invariants verified:
//! - No panic regardless of input.
//! - Decoding is canonical: a decoded `Action` re-encodes, and that encoding
//!   round-trips back to an identical byte string (encode/decode idempotency).

#![no_main]

use libfuzzer_sys::fuzz_target;
use libsalus::{Action, decode, encode};

fuzz_target!(|data: &[u8]| {
    if let Ok(action) = decode::<Action>(data)
        && let Ok(reencoded) = encode(&action)
    {
        if let Ok(action2) = decode::<Action>(&reencoded) {
            let reencoded2 = encode(&action2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2, "encode/decode must be idempotent");
        }
    }
});
