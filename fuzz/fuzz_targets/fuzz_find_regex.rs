// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the daemon's `find` key search.
//!
//! `ShareStore::find` compiles a caller-supplied pattern with `Regex::new` and
//! matches it against every stored key. The pattern is the only
//! attacker-controlled input on this path, so `salusd::fuzz::find_regex` drives
//! it against a store seeded with representative keys. An invalid pattern must
//! surface as an `Err`, never a panic.
//!
//! Invariants verified:
//! - No panic regardless of input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use salusd::fuzz::find_regex;

fuzz_target!(|data: &[u8]| {
    let pattern = String::from_utf8_lossy(data);
    // All outcomes (Ok matches or an Err for an invalid pattern) are fine.
    let _ = find_regex(&pattern);
});
