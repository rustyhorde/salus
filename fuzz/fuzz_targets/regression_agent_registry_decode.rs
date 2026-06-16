// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_agent_registry_decode` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_agent_registry_decode/`.

use salus_agent::keystore::decode_registry;

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_agent_registry_decode(data: &[u8]) {
    // All outcomes (Ok or Err) are acceptable; only panics are failures.
    let _ = decode_registry(data);
}

#[test]
fn regression_empty_and_short() {
    run_fuzz_agent_registry_decode(&[]);
    run_fuzz_agent_registry_decode(&[0]);
    run_fuzz_agent_registry_decode(&[0xff]);
}

#[test]
fn regression_forged_length_prefix_is_bounded() {
    // A claimed multi-gigabyte `sets` vector must be rejected by the size limit,
    // not drive an allocation.
    run_fuzz_agent_registry_decode(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
}
