// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use bincode::{Decode, Encode};
use bon::Builder;
use getset::CopyGetters;

/// The init message to send to the daemon
#[derive(Builder, Clone, Copy, CopyGetters, Debug, Decode, Encode)]
#[getset(get_copy = "pub")]
pub struct Init {
    /// The number of shares to create
    #[builder(default = 5)]
    num_shares: u8,
    /// The minimum number of shares needed to reconstruct the key
    #[builder(default = 3)]
    threshold: u8,
}

/// A share message to send to the daemon
#[derive(Builder, Clone, Debug, Decode, Encode)]
pub struct Share {
    #[builder(into)]
    share: String,
}

impl Share {
    /// Get the share
    #[must_use]
    pub fn share(&self) -> &str {
        &self.share
    }
}

/// A share message to send to the daemon
#[derive(Builder, Clone, Debug, Decode, Encode)]
pub struct Shares {
    #[builder(into)]
    shares: Vec<String>,
}

impl Shares {
    /// Get the shares
    #[must_use]
    pub fn shares(&self) -> &[String] {
        &self.shares
    }
}

/// A store message to send to the daemon
#[derive(Builder, Clone, Debug, Decode, Encode)]
pub struct Store {
    #[builder(into)]
    key: String,
    #[builder(into)]
    value: String,
}

impl Store {
    /// Get the key
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Get the value
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Get the key and value as a tuple
    #[must_use]
    pub fn into_parts(self) -> (String, String) {
        (self.key, self.value)
    }
}

/// A message to send to the daemon
#[derive(Clone, Debug, Decode, Encode)]
pub enum Action {
    /// Attempt to unlock the store
    Unlock,
    /// Send a share to the daemon
    Share(Share),
    /// Generate the salus shares
    GenShares(u8, u8),
    /// Store an encrypted value
    Store(Store),
    /// Read an encrypted value
    Read(String),
    /// Get the threshold
    GetThreshold,
}

/// A response from the daemon
#[derive(Clone, Debug, Decode, Encode)]
pub enum Response {
    /// Error
    Error(String),
    /// Success
    Success,
    /// Shares
    Shares(Shares),
    /// The share store is already initialized
    AlreadyInitialiazed,
    /// The threshold
    Threshold(u8),
    /// The value read from the store
    Value(Option<String>),
    /// The key was not found in the store
    KeyNotFound,
}
