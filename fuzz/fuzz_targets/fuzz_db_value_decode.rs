// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the daemon's redb value deserialization.
//!
//! `ConfigVal` and `SalusVal` (`salusd/src/db/values/`) implement redb's
//! infallible `Value::from_bytes`, which panicked on truncated or garbage bytes
//! (an `.unwrap()` and an out-of-bounds nonce slice). The real parse now lives
//! in fallible `try_from_bytes` helpers; this target drives both via the
//! `salusd::fuzz` facades on arbitrary bytes to prove they return `Err` rather
//! than panicking. redb only ever feeds `from_bytes` its own output, but a
//! corrupted on-disk database could supply anything.
//!
//! Invariants verified:
//! - No panic regardless of input (`Ok`/`Err` both fine).

#![no_main]

use libfuzzer_sys::fuzz_target;
use salusd::fuzz::{decode_config_val, decode_salus_val};

fuzz_target!(|data: &[u8]| {
    let _ = decode_config_val(data);
    let _ = decode_salus_val(data);
});
