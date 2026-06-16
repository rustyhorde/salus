// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Fuzz target for the daemon's redb value deserialization.
//!
//! `ConfigVal` and `SalusVal` (`salusd/src/db/values/`) wrap their raw redb
//! bytes verbatim, so `Value::from_bytes` is infallible and can never panic. The
//! real fallible decode lives downstream: `ConfigVal::to_value` (bincode) and
//! `SalusVal::nonce` (the 12-byte nonce split). This target drives both via the
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
