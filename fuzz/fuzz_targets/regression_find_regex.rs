// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Regression tests for `fuzz_find_regex` crashes.
//!
//! See `regression_action_decode.rs` for the workflow used to add new crashes;
//! commit raw reproducers under `fuzz/artifacts/fuzz_find_regex/`.

use salusd::fuzz::find_regex;

/// Mirrors the fuzz target body exactly. Any panic here is a confirmed bug.
fn run_fuzz_find_regex(data: &[u8]) {
    let pattern = String::from_utf8_lossy(data);
    let _ = find_regex(&pattern);
}

#[test]
fn regression_empty_and_simple() {
    run_fuzz_find_regex(&[]);
    run_fuzz_find_regex(b".*");
    run_fuzz_find_regex(b"alpha");
}

#[test]
fn regression_invalid_patterns() {
    // Unbalanced/invalid regexes must return Err, not panic.
    run_fuzz_find_regex(b"(");
    run_fuzz_find_regex(b"[");
    run_fuzz_find_regex(b"*");
    run_fuzz_find_regex(b"\\");
}

#[test]
fn regression_matches_seed_keys() {
    // Patterns that match the seeded keys exercise the match-collection path.
    let matches = find_regex("db/.*").expect("valid pattern");
    assert!(matches.iter().any(|k| k == "db/prod/password"));
}
