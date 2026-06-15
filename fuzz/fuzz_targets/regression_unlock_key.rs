// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_unlock_key` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_unlock_key/`.

use libsalus::unlock_key;

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_unlock_key(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let shares: Vec<String> = text.split('\n').map(ToString::to_string).collect();
    let _ = unlock_key(&shares);
}

#[test]
fn regression_empty_and_garbage() {
    run_fuzz_unlock_key(&[]);
    run_fuzz_unlock_key(b"");
    run_fuzz_unlock_key(b"\n\n\n");
    run_fuzz_unlock_key(b"not-a-share");
}

#[test]
fn regression_malformed_hex_shares() {
    // Share-shaped lines with invalid hex / index bytes must error, not panic.
    run_fuzz_unlock_key(b"1-zzzz\n2-yyyy\n3-xxxx");
    run_fuzz_unlock_key(b"00\n01\n02");
}
