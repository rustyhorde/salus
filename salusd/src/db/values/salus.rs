// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::{Result, anyhow};
use redb::{TypeName, Value};

/// A `salus_store` row: an AES-256-GCM nonce followed by its ciphertext.
///
/// `SalusVal` is a thin newtype over the raw `nonce || ciphertext` bytes, stored
/// in `redb` verbatim. The infallible [`Value::from_bytes`] / [`Value::as_bytes`]
/// hooks are therefore genuine no-op wraps/unwraps that can never panic on a
/// corrupt database. Splitting the 12-byte nonce off the front is fallible (a
/// truncated row has fewer than 12 bytes) and lives in [`SalusVal::nonce`] /
/// [`SalusVal::ciphertext`], which are called where an `Err` can be propagated.
#[derive(Clone, Debug)]
pub(crate) struct SalusVal {
    /// `nonce` (12 bytes) followed by the ciphertext.
    raw: Vec<u8>,
}

/// Length of the AES-256-GCM nonce that prefixes every stored value.
const NONCE_LEN: usize = 12;

impl SalusVal {
    /// Build a `SalusVal` from a freshly-sealed nonce and ciphertext.
    pub(crate) fn from_parts(nonce: [u8; NONCE_LEN], ciphertext: &[u8]) -> Self {
        let mut raw = Vec::with_capacity(NONCE_LEN.saturating_add(ciphertext.len()));
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(ciphertext);
        Self { raw }
    }

    /// Wrap raw stored bytes as a `SalusVal` without validating them.
    ///
    /// Infallible: validation (the 12-byte nonce split) is deferred to
    /// [`SalusVal::nonce`] / [`SalusVal::ciphertext`].
    pub(crate) fn from_raw_bytes(data: &[u8]) -> Self {
        Self { raw: data.to_vec() }
    }

    /// Split the row into its nonce and ciphertext, erroring on a truncated row.
    fn split(&self) -> Result<(&[u8; NONCE_LEN], &[u8])> {
        self.raw
            .split_first_chunk::<NONCE_LEN>()
            .ok_or_else(|| anyhow!("SalusVal is malformed (need at least {NONCE_LEN} nonce bytes)"))
    }

    /// The 12-byte AES-256-GCM nonce.
    pub(crate) fn nonce(&self) -> Result<[u8; NONCE_LEN]> {
        Ok(*self.split()?.0)
    }

    /// The ciphertext following the nonce.
    pub(crate) fn ciphertext(&self) -> Result<&[u8]> {
        Ok(self.split()?.1)
    }
}

impl Value for SalusVal {
    type SelfType<'a>
        = SalusVal
    where
        Self: 'a;

    type AsBytes<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        SalusVal::from_raw_bytes(data)
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        value.raw.as_slice()
    }

    fn type_name() -> TypeName {
        TypeName::new("SalusVal")
    }
}
