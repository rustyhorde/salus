// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::Result;
use bincode::{
    Decode, Encode, config::standard, decode_from_slice, encode_into_slice, encode_to_vec,
};
use bon::Builder;
use getset::Getters;
use redb::{TypeName, Value};

#[derive(Builder, Clone, Debug, Decode, Encode, Getters)]
pub(crate) struct ConfigVal {
    value: Vec<u8>,
}

impl ConfigVal {
    pub(crate) fn to_value<T: Decode<()>>(&self) -> Result<T> {
        let (res, _): (T, usize) = decode_from_slice(&self.value, standard())?;
        Ok(res)
    }

    pub(crate) fn from_value<T: Encode>(value: T) -> Result<Self> {
        let value = encode_to_vec(&value, standard())?;
        Ok(Self::builder().value(value).build())
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
        let res: (ConfigVal, usize) = decode_from_slice(data, standard()).unwrap();
        res.0
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        let mut bytes = [0u8; 100];
        let length = encode_into_slice(value, &mut bytes, standard()).unwrap();
        bytes[..length].to_vec()
    }

    fn type_name() -> TypeName {
        TypeName::new("ConfigVal")
    }
}
