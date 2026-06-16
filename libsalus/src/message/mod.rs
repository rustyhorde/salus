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
use getset::CopyGetters;

pub(crate) mod agent;

/// Maximum size, in bytes, of a single encoded protocol message (1 MiB).
///
/// Both the daemon and the client refuse to decode or allocate beyond this
/// bound, so a hostile peer cannot trigger an unbounded allocation by forging a
/// large length prefix in an otherwise tiny message.
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Encode a protocol message using the shared, size-bounded wire configuration.
///
/// # Errors
///
/// Returns an error if encoding fails or the encoded form would exceed
/// [`MAX_MESSAGE_SIZE`].
pub fn encode<E: Encode>(message: E) -> Result<Vec<u8>> {
    Ok(encode_to_vec(
        message,
        standard().with_limit::<MAX_MESSAGE_SIZE>(),
    )?)
}

/// Decode a protocol message using the shared, size-bounded wire configuration.
///
/// The size limit ensures a forged length prefix cannot drive an unbounded
/// allocation; oversized or otherwise malformed input is rejected with an error
/// instead.
///
/// # Errors
///
/// Returns an error if the bytes are not a valid encoding of `D` or would
/// exceed [`MAX_MESSAGE_SIZE`].
pub fn decode<D: Decode<()>>(bytes: &[u8]) -> Result<D> {
    let (message, _len) = decode_from_slice(bytes, standard().with_limit::<MAX_MESSAGE_SIZE>())?;
    Ok(message)
}

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

/// How long the daemon should keep the reconstructed key in memory after a
/// successful unlock.
#[derive(Clone, Copy, Debug, Decode, Default, Encode, Eq, PartialEq)]
pub enum UnlockTimeout {
    /// Use the daemon's configured `key_timeout` default.
    #[default]
    Default,
    /// Keep the key for this many seconds. The daemon clamps the value to at
    /// most 24 hours (`86_400` seconds).
    Seconds(u64),
    /// Keep the key until an explicit lock or the daemon restarts; no
    /// auto-clear timer is armed.
    Forever,
}

/// The maximum number of seconds the daemon will hold an unlocked key (24 h).
pub const MAX_UNLOCK_SECONDS: u64 = 24 * 60 * 60;

/// A message to send to the daemon
#[derive(Clone, Debug, Decode, Encode)]
pub enum Action {
    /// Attempt to unlock the store, holding the key for the given duration
    Unlock(UnlockTimeout),
    /// Clear the unlocked key (and any pending auto-clear timer) immediately
    Lock,
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
    /// Find a key
    FindKey(String),
}

/// A response from the daemon
#[derive(Clone, Debug, Decode, Encode)]
pub enum Response {
    /// Error
    Error(String),
    /// Success
    Success,
    /// The store could not be unlocked with the provided shares
    UnlockFailed,
    /// Shares
    Shares(Shares),
    /// The share store is already initialized
    AlreadyInitialiazed,
    /// The threshold
    Threshold(u8),
    /// The value read from the store
    Value(Option<Vec<u8>>),
    /// The key was not found in the store
    KeyNotFound,
    /// The keys that matched the regex
    Matches(Vec<String>),
}
