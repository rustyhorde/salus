// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

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
        let nonce = data[0..12].try_into().expect("slice with incorrect length");
        let ciphertext = data[12..].to_vec();
        SalusVal { nonce, ciphertext }
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
