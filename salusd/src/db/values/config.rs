// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::Result;
use bincode_next::{Decode, Encode, config::standard, decode_from_slice, encode_to_vec};
use bon::Builder;
use getset::Getters;
use libsalus::MAX_MESSAGE_SIZE;
use redb::{TypeName, Value};

#[derive(Builder, Clone, Debug, Decode, Encode, Getters)]
pub(crate) struct ConfigVal {
    value: Vec<u8>,
}

impl ConfigVal {
    pub(crate) fn to_value<T: Decode<()>>(&self) -> Result<T> {
        let (res, _): (T, usize) =
            decode_from_slice(&self.value, standard().with_limit::<MAX_MESSAGE_SIZE>())?;
        Ok(res)
    }

    pub(crate) fn from_value<T: Encode>(value: T) -> Result<Self> {
        let value = encode_to_vec(&value, standard())?;
        Ok(Self::builder().value(value).build())
    }

    /// Fallible counterpart to the infallible redb [`Value::from_bytes`] hook.
    ///
    /// redb only ever calls `from_bytes` on bytes it previously produced via
    /// `as_bytes`, but a corrupted database (or a fuzzer) can supply arbitrary
    /// bytes. Keeping the real parse here lets it return an `Err` instead of
    /// panicking, so it can be exercised directly.
    pub(crate) fn try_from_bytes(data: &[u8]) -> Result<ConfigVal> {
        let (res, _): (ConfigVal, usize) =
            decode_from_slice(data, standard().with_limit::<MAX_MESSAGE_SIZE>())?;
        Ok(res)
    }
}

impl Value for ConfigVal {
    type SelfType<'a>
        = ConfigVal
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
        Self::try_from_bytes(data).expect("ConfigVal decode")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        encode_to_vec(value, standard()).expect("ConfigVal encode")
    }

    fn type_name() -> TypeName {
        TypeName::new("ConfigVal")
    }
}
