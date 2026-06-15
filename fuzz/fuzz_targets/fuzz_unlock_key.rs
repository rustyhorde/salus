// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for `unlock_key` (Shamir secret reconstruction).
//!
//! `unlock_key` (`libsalus/src/key/mod.rs`) parses untrusted share strings and
//! reconstructs a key via the `ssss` crate. Shares arrive from the operator
//! during the unlock ceremony, so a typo'd, truncated, or hostile share must be
//! rejected with an `Err` rather than panicking the daemon.
//!
//! The fuzzer bytes are split on newlines into individual share strings,
//! mirroring how a caller collects shares line-by-line.
//!
//! Invariants verified:
//! - No panic regardless of input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use libsalus::unlock_key;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let shares: Vec<String> = text.split('\n').map(ToString::to_string).collect();
    // All outcomes (Ok or Err) are acceptable; only panics are failures.
    let _ = unlock_key(&shares);
});
