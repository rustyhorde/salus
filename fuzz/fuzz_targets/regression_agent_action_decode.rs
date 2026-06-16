// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_agent_action_decode` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_agent_action_decode/`.

use libsalus::{AgentAction, decode, encode};

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_agent_action_decode(data: &[u8]) {
    if let Ok(action) = decode::<AgentAction>(data)
        && let Ok(reencoded) = encode(&action)
    {
        if let Ok(action2) = decode::<AgentAction>(&reencoded) {
            let reencoded2 = encode(&action2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2);
        }
    }
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_agent_action_decode(&[]);
    run_fuzz_agent_action_decode(&[0]);
    run_fuzz_agent_action_decode(&[0xff]);
}

#[test]
fn regression_oversized_variant_tag() {
    // A leading byte beyond the highest `AgentAction` discriminant must not panic.
    run_fuzz_agent_action_decode(&[0x7f, 0x00, 0x00, 0x00]);
}

#[test]
fn regression_valid_actions_round_trip() {
    // Each well-formed variant re-encodes identically.
    for action in [
        AgentAction::Status,
        AgentAction::GetAutoShares {
            set: "alpha".to_string(),
        },
        AgentAction::UnsealFinal {
            set: "alpha".to_string(),
            passphrase: "correct horse battery staple".to_string(),
        },
        AgentAction::Lock { set: None },
        AgentAction::Lock {
            set: Some("alpha".to_string()),
        },
    ] {
        let encoded = encode(&action).expect("encode");
        run_fuzz_agent_action_decode(&encoded);
    }
}

#[test]
fn regression_forged_length_prefix_is_bounded() {
    // A tiny message whose length prefix claims a multi-gigabyte payload must be
    // rejected by the size limit, not drive an allocation.
    run_fuzz_agent_action_decode(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
}
