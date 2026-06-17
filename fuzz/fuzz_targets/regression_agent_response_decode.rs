// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_agent_response_decode` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_agent_response_decode/`.

use libsalus::{AgentResponse, SetInfo, decode, encode};

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_agent_response_decode(data: &[u8]) {
    if let Ok(response) = decode::<AgentResponse>(data)
        && let Ok(reencoded) = encode(&response)
    {
        if let Ok(response2) = decode::<AgentResponse>(&reencoded) {
            let reencoded2 = encode(&response2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2);
        }
    }
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_agent_response_decode(&[]);
    run_fuzz_agent_response_decode(&[0]);
    run_fuzz_agent_response_decode(&[0xff]);
}

#[test]
fn regression_oversized_variant_tag() {
    // A leading byte beyond the highest `AgentResponse` discriminant must not panic.
    run_fuzz_agent_response_decode(&[0x7f, 0x00, 0x00, 0x00]);
}

#[test]
fn regression_valid_responses_round_trip() {
    // Each well-formed variant re-encodes identically.
    for response in [
        AgentResponse::Status {
            sets: vec![SetInfo {
                name: "alpha".to_string(),
                auto_count: 2,
            }],
        },
        AgentResponse::AutoShares(vec!["share-0".to_string(), "share-1".to_string()]),
        AgentResponse::FinalShare("share-2".to_string()),
        AgentResponse::UnknownSet,
        AgentResponse::Unenrolled,
        AgentResponse::BadPassphrase,
        AgentResponse::Error("boom".to_string()),
    ] {
        let encoded = encode(&response).expect("encode");
        run_fuzz_agent_response_decode(&encoded);
    }
}

#[test]
fn regression_forged_length_prefix_is_bounded() {
    // A tiny message whose length prefix claims a multi-gigabyte payload must be
    // rejected by the size limit, not drive an allocation.
    run_fuzz_agent_response_decode(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
}
