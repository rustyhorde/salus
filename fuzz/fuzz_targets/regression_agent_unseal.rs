// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_agent_unseal` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_agent_unseal/`.

use salus_agent::keystore::unseal;

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_agent_unseal(data: &[u8]) {
    let Some((pass_len, rest)) = data.split_first() else {
        return;
    };
    let split = (*pass_len as usize).min(rest.len());
    let (pass_bytes, blob) = rest.split_at(split);
    let passphrase = String::from_utf8_lossy(pass_bytes);

    // All outcomes (Ok or Err) are acceptable; only panics are failures.
    let _ = unseal(blob, &passphrase);
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_agent_unseal(&[]);
    run_fuzz_agent_unseal(&[0]);
    // pass_len = 0, blob too short for the salt+nonce header → Err, no panic.
    run_fuzz_agent_unseal(&[0x00, 0x01, 0x02, 0x03]);
}

#[test]
fn regression_blob_at_minimum_length() {
    // pass_len = 2 passphrase bytes, then exactly the 16-byte salt + 12-byte
    // nonce header with empty ciphertext. The open fails (wrong key) → Ok(None).
    let mut data = vec![0x02, b'h', b'i'];
    data.extend_from_slice(&[0u8; 28]);
    run_fuzz_agent_unseal(&data);
}
