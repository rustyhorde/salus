// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::path::PathBuf;

use anyhow::Result;

#[allow(clippy::unnecessary_wraps)]
pub(crate) fn to_path_buf(path: &String) -> Result<PathBuf> {
    Ok(PathBuf::from(path))
}
