// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the `salus-agent`'s keyring registry deserialization.
//!
//! At startup the agent runs `decode::<Registry>` over bytes loaded from the OS
//! keyring (`salus-agent/src/keystore/mod.rs`). The keyring is at-rest
//! encryption, but corrupted or tampered bytes must be rejected with an `Err`,
//! never a panic. This drives the same size-bounded `libsalus::decode` path via
//! the `fuzzing`-gated `keystore::decode_registry` facade.
//!
//! Invariants verified:
//! - No panic regardless of input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use salus_agent::keystore::decode_registry;

fuzz_target!(|data: &[u8]| {
    // All outcomes (Ok or Err) are acceptable; only panics are failures.
    let _ = decode_registry(data);
});
