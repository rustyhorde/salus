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
use libsalus::MAX_MESSAGE_SIZE;
use redb::{TypeName, Value};

/// A `salus_config` row.
///
/// `ConfigVal` is a thin newtype over the bincode-encoded bytes of a single
/// config value (a `bool` or `u8`). It is stored in `redb` verbatim, so the
/// infallible [`Value::from_bytes`] / [`Value::as_bytes`] hooks are genuine
/// no-op wraps/unwraps that can never panic on a corrupt database. The real
/// fallible decode lives in [`ConfigVal::to_value`], which is called at the use
/// site where an `Err` can be propagated.
#[derive(Builder, Clone, Debug)]
pub(crate) struct ConfigVal {
    value: Vec<u8>,
}

impl ConfigVal {
    /// Decode the wrapped bytes into a concrete config value `T`.
    pub(crate) fn to_value<T: Decode<()>>(&self) -> Result<T> {
        let (res, _): (T, usize) =
            decode_from_slice(&self.value, standard().with_limit::<MAX_MESSAGE_SIZE>())?;
        Ok(res)
    }

    /// Encode a concrete config value `T` into a `ConfigVal`.
    pub(crate) fn from_value<T: Encode>(value: T) -> Result<Self> {
        let value = encode_to_vec(&value, standard())?;
        Ok(Self::builder().value(value).build())
    }

    /// Wrap raw stored bytes as a `ConfigVal` without interpreting them.
    ///
    /// Infallible: any byte string is a valid wrapped payload. Interpretation
    /// happens later (and fallibly) via [`ConfigVal::to_value`].
    pub(crate) fn from_raw_bytes(data: &[u8]) -> Self {
        Self::builder().value(data.to_vec()).build()
    }
}

impl Value for ConfigVal {
    type SelfType<'a>
        = ConfigVal
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
        ConfigVal::from_raw_bytes(data)
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        value.value.as_slice()
    }

    fn type_name() -> TypeName {
        TypeName::new("ConfigVal")
    }
}
