// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the client's wire-protocol decode of `AgentResponse`.
//!
//! The CLI client runs `decode_from_slice::<AgentResponse>` over bytes read from
//! the agent socket (`salusc/src/inter/mod.rs`). A compromised or buggy agent
//! (or anything able to write to the socket) must not be able to panic the
//! client, so every byte string must decode to an `Err` or a well-formed
//! `AgentResponse`.
//!
//! Like `fuzz_agent_action_decode`, this drives the size-bounded
//! `libsalus::decode`/`encode` helpers, so a forged length prefix is rejected
//! rather than triggering an unbounded allocation.
//!
//! Invariants verified:
//! - No panic regardless of input.
//! - Decoding is canonical (encode/decode idempotency).

#![no_main]

use libfuzzer_sys::fuzz_target;
use libsalus::{AgentResponse, decode, encode};

fuzz_target!(|data: &[u8]| {
    if let Ok(response) = decode::<AgentResponse>(data)
        && let Ok(reencoded) = encode(&response)
    {
        if let Ok(response2) = decode::<AgentResponse>(&reencoded) {
            let reencoded2 = encode(&response2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2, "encode/decode must be idempotent");
        }
    }
});
