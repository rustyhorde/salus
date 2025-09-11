// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use bincode::{Decode, Encode};
use bon::Builder;

/// The init message to send to the daemon
#[derive(Builder, Clone, Copy, Debug, Decode, Encode)]
pub struct Init {
    #[builder(default = 5)]
    num_shares: u8,
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

/// A message to send to the daemon
#[derive(Clone, Debug, Decode, Encode)]
pub enum Action {
    /// Init
    Init(Init),
    /// Unlock
    Unlock,
    /// Share
    Share(Share),
    /// Genkey
    Genkey,
}

/// A response from the daemon
#[derive(Clone, Debug, Decode, Encode)]
pub enum Response {
    /// Error
    Error,
    /// Success
    Success,
    /// Shares
    Shares(Shares),
}
