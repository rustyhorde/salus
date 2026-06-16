// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the `salus-agent`'s passphrase-sealed share parser.
//!
//! `keystore::unseal` (`salus-agent/src/keystore/mod.rs`) hand-parses a
//! `salt || nonce || ciphertext` blob loaded from the OS keyring, derives a key
//! with argon2id, and AES-256-GCM opens it. A malformed blob (too short, bad
//! nonce length, non-UTF-8 plaintext) must return `Err`, a wrong passphrase must
//! return `Ok(None)`, and neither must panic.
//!
//! The first input byte selects how many of the remaining bytes form the
//! passphrase (lossy UTF-8); the rest is the sealed blob. The cheap "too short"
//! guard is exercised heavily; longer inputs run argon2id once, so this target
//! has lower throughput than the pure-decode targets.
//!
//! Invariants verified:
//! - No panic regardless of input (`Ok(Some)` / `Ok(None)` / `Err` all fine).

#![no_main]

use libfuzzer_sys::fuzz_target;
use salus_agent::keystore::unseal;

fuzz_target!(|data: &[u8]| {
    let Some((pass_len, rest)) = data.split_first() else {
        return;
    };
    let split = (*pass_len as usize).min(rest.len());
    let (pass_bytes, blob) = rest.split_at(split);
    let passphrase = String::from_utf8_lossy(pass_bytes);

    // All outcomes (Ok or Err) are acceptable; only panics are failures.
    let _ = unseal(blob, &passphrase);
});
