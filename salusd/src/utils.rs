// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{
    fs::create_dir_all,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::error::Error;

#[allow(clippy::unnecessary_wraps)]
pub(crate) fn to_path_buf(path: &String) -> Result<PathBuf> {
    Ok(PathBuf::from(path))
}

/// Create the parent directory of `path` if it does not already exist.
///
/// The per-user `dirs2` locations the daemon defaults to are not guaranteed to
/// exist, so any file we create there (database, log) needs its parent created
/// first.
pub(crate) fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent).with_context(|| Error::CreateDir)?;
    }
    Ok(())
}
