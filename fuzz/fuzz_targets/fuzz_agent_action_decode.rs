// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the `salus-agent`'s wire-protocol decode of `AgentAction`.
//!
//! The agent runs `decode_from_slice::<AgentAction>` over raw bytes read from
//! its IPC socket (`salus-agent/src/runtime/mod.rs`), so this is the most
//! attacker-controlled input the agent sees. A malformed or hostile client
//! message must be rejected with an `Err`, never a panic.
//!
//! Like `fuzz_action_decode`, this drives the size-bounded `libsalus::decode`/
//! `encode` helpers, so a forged length prefix is rejected rather than
//! triggering an unbounded allocation.
//!
//! Invariants verified:
//! - No panic regardless of input.
//! - Decoding is canonical (encode/decode idempotency).

#![no_main]

use libfuzzer_sys::fuzz_target;
use libsalus::{AgentAction, decode, encode};

fuzz_target!(|data: &[u8]| {
    if let Ok(action) = decode::<AgentAction>(data)
        && let Ok(reencoded) = encode(&action)
    {
        if let Ok(action2) = decode::<AgentAction>(&reencoded) {
            let reencoded2 = encode(&action2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2, "encode/decode must be idempotent");
        }
    }
});
