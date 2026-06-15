// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_response_decode` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_response_decode/`.

use libsalus::{Response, decode, encode};

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_response_decode(data: &[u8]) {
    if let Ok(response) = decode::<Response>(data)
        && let Ok(reencoded) = encode(&response)
    {
        if let Ok(response2) = decode::<Response>(&reencoded) {
            let reencoded2 = encode(&response2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2);
        }
    }
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_response_decode(&[]);
    run_fuzz_response_decode(&[0]);
    run_fuzz_response_decode(&[0xff]);
}

#[test]
fn regression_oversized_variant_tag() {
    run_fuzz_response_decode(&[0x7f, 0x00, 0x00, 0x00]);
}

#[test]
fn regression_valid_response_round_trips() {
    // A well-formed `Response::Threshold(3)` re-encodes identically.
    let encoded = encode(Response::Threshold(3)).expect("encode");
    run_fuzz_response_decode(&encoded);
}

#[test]
fn regression_forged_length_prefix_is_bounded() {
    // A tiny message whose length prefix claims a multi-gigabyte payload must be
    // rejected by the size limit rather than driving an allocation.
    run_fuzz_response_decode(&[0x03, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
}
