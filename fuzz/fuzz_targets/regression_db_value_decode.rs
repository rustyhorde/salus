// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_db_value_decode` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_db_value_decode/`.

use salusd::fuzz::{decode_config_val, decode_salus_val};

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_db_value_decode(data: &[u8]) {
    let _ = decode_config_val(data);
    let _ = decode_salus_val(data);
}

#[test]
fn regression_empty_and_short() {
    // Fewer than 12 bytes used to panic the `SalusVal` nonce slice; it must now
    // return `Err`.
    run_fuzz_db_value_decode(&[]);
    run_fuzz_db_value_decode(&[0]);
    run_fuzz_db_value_decode(&[0u8; 11]);
}

#[test]
fn regression_salus_val_at_minimum_length() {
    // Exactly 12 bytes → a valid `SalusVal` with an empty ciphertext.
    run_fuzz_db_value_decode(&[0u8; 12]);
    // More than 12 bytes → nonce plus a non-empty ciphertext.
    run_fuzz_db_value_decode(&[0xab; 32]);
}

#[test]
fn regression_config_val_forged_length_prefix_is_bounded() {
    // A tiny `ConfigVal` whose inner `Vec<u8>` length prefix claims a multi-
    // gigabyte payload must be rejected by the size limit, not drive an
    // unbounded allocation. (Regression for the OOM the fuzzer found in the
    // unbounded `standard()` decode of `ConfigVal`.)
    run_fuzz_db_value_decode(&[0xfc, 0x2e, 0xdd, 0xdd, 0xdd]);
}
