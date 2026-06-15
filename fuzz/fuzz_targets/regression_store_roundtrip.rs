// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_store_roundtrip` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_store_roundtrip/`.

use salusd::fuzz::store_roundtrip;

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_store_roundtrip(data: &[u8]) {
    let Some((key_len, rest)) = data.split_first() else {
        return;
    };
    let split = (*key_len as usize).min(rest.len());
    let (key_bytes, value) = rest.split_at(split);
    let key = String::from_utf8_lossy(key_bytes);

    if let Ok(Some(plaintext)) = store_roundtrip(&key, value) {
        assert_eq!(plaintext, value, "store/read round-trip must preserve the value");
    }
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_store_roundtrip(&[]);
    run_fuzz_store_roundtrip(&[0]);
    run_fuzz_store_roundtrip(&[0, 0]);
}

#[test]
fn regression_empty_key_and_value() {
    // key_len = 0 → empty key, empty value.
    run_fuzz_store_roundtrip(&[0x00]);
    // key_len larger than remaining → whole rest is the key, empty value.
    run_fuzz_store_roundtrip(&[0xff, b'k', b'e', b'y']);
}

#[test]
fn regression_binary_value() {
    // 1-byte key "k", value is the remaining non-UTF-8 bytes.
    run_fuzz_store_roundtrip(&[0x01, b'k', 0x00, 0xff, 0xfe, 0x80]);
}
