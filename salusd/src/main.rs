// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! salus daemon

// The daemon logic lives in the `salusd` library; this binary is a thin
// entry point. The library target owns every dependency, so silence the
// `unused_crate_dependencies` lint for the binary target only.
#![cfg_attr(nightly, allow(unused_crate_dependencies))]

use std::process;

#[tokio::main]
async fn main() {
    process::exit(salusd::run_daemon().await)
}
