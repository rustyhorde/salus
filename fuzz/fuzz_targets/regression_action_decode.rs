// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_action_decode` crashes.
//!
//! Per the cargo-fuzz documentation, each crash is embedded here as a `&[u8]`
//! constant so that `cargo test` permanently guards against regressions without
//! requiring a nightly fuzzer run.
//!
//! To add a new crash:
//! 1. Extract the bytes from the artifact downloaded from CI.
//! 2. Run `xxd -i crash-<hash>` (or `hexdump -C`) to get the byte values.
//! 3. Add a new test function following the pattern below.
//! 4. Commit the raw crash file to `fuzz/artifacts/fuzz_action_decode/crash-<hash>`
//!    so `cargo +nightly fuzz run fuzz_action_decode` also replays it.

use libsalus::{Action, decode, encode};

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_action_decode(data: &[u8]) {
    if let Ok(action) = decode::<Action>(data)
        && let Ok(reencoded) = encode(&action)
    {
        if let Ok(action2) = decode::<Action>(&reencoded) {
            let reencoded2 = encode(&action2).expect("re-encode must succeed");
            assert_eq!(reencoded, reencoded2);
        }
    }
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_action_decode(&[]);
    run_fuzz_action_decode(&[0]);
    run_fuzz_action_decode(&[0xff]);
}

#[test]
fn regression_oversized_variant_tag() {
    // A leading byte beyond the highest `Action` discriminant must not panic.
    run_fuzz_action_decode(&[0x7f, 0x00, 0x00, 0x00]);
}

#[test]
fn regression_valid_action_round_trips() {
    // A well-formed `Action::GenShares(5, 3)` re-encodes identically.
    let encoded = encode(Action::GenShares(5, 3)).expect("encode");
    run_fuzz_action_decode(&encoded);
}

#[test]
fn regression_forged_length_prefix_is_bounded() {
    // A tiny message whose length prefix claims a multi-gigabyte payload must be
    // rejected by the size limit, not drive an allocation. (Regression for the
    // OOM the fuzzer found in the unbounded `standard()` decode.)
    run_fuzz_action_decode(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
}
