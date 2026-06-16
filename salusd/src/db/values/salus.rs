// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::{Result, bail};
use bon::Builder;
use getset::{CopyGetters, Getters};
use redb::{TypeName, Value};

#[derive(Builder, Clone, CopyGetters, Debug, Getters)]
pub(crate) struct SalusVal {
    #[builder(into)]
    #[getset(get_copy = "pub(crate)")]
    nonce: [u8; 12],
    #[builder(into)]
    #[getset(get = "pub(crate)")]
    ciphertext: Vec<u8>,
}

impl SalusVal {
    /// Fallible counterpart to the infallible redb [`Value::from_bytes`] hook.
    ///
    /// redb only ever calls `from_bytes` on bytes it previously produced via
    /// `as_bytes`, but a corrupted database (or a fuzzer) can supply fewer than
    /// the 12 nonce bytes. Keeping the real parse here lets it return an `Err`
    /// instead of panicking on the slice, so it can be exercised directly.
    pub(crate) fn try_from_bytes(data: &[u8]) -> Result<SalusVal> {
        let Some((nonce_bytes, ciphertext)) = data.split_at_checked(12) else {
            bail!("SalusVal is malformed (need at least 12 nonce bytes)");
        };
        let nonce = nonce_bytes
            .try_into()
            .expect("split_at_checked(12) yields exactly 12 bytes");
        Ok(SalusVal {
            nonce,
            ciphertext: ciphertext.to_vec(),
        })
    }
}

impl Value for SalusVal {
    type SelfType<'a>
        = SalusVal
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        Self::try_from_bytes(data).expect("SalusVal decode")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        let mut bytes = Vec::with_capacity(12 + value.ciphertext.len());
        bytes.extend_from_slice(&value.nonce);
        bytes.extend_from_slice(&value.ciphertext);
        bytes
    }

    fn type_name() -> TypeName {
        TypeName::new("SalusVal")
    }
}
