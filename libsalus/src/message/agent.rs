// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! The wire protocol spoken between the client and the `salus-agent`.
//!
//! The agent holds, for each enrolled *set*, the `threshold - 1` automatically
//! retrievable shares (loaded from the OS keyring at login) and the single
//! passphrase-sealed final share. The client queries the agent for the auto
//! shares and asks it to unseal the final share with a user-supplied passphrase,
//! then forwards every share to the daemon to unlock the store.

use bincode_next::{Decode, Encode};

/// Summary of a single enrolled set, returned by [`AgentAction::Status`].
#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
pub struct SetInfo {
    /// The name of the enrolled set.
    pub name: String,
    /// How many automatically retrievable shares the set holds. The number of
    /// passphrase-sealed shares is always exactly one.
    pub auto_count: u8,
}

/// A request sent from the client to the `salus-agent`.
#[derive(Clone, Debug, Decode, Encode)]
pub enum AgentAction {
    /// List the names of every enrolled set.
    Status,
    /// Get a set's `threshold - 1` automatically retrievable shares.
    GetAutoShares {
        /// The name of the set to read.
        set: String,
    },
    /// Unseal a set's single passphrase-protected share.
    UnsealFinal {
        /// The name of the set to unseal.
        set: String,
        /// The passphrase guarding the set's final share.
        passphrase: String,
    },
    /// Drop any cached unsealed share. `None` clears every set's cache.
    Lock {
        /// The set whose cache to clear, or `None` for all sets.
        set: Option<String>,
    },
}

/// A response sent from the `salus-agent` back to the client.
#[derive(Clone, Debug, Decode, Encode)]
pub enum AgentResponse {
    /// The enrolled sets (empty when nothing is enrolled).
    Status {
        /// One entry per enrolled set.
        sets: Vec<SetInfo>,
    },
    /// A set's automatically retrievable shares.
    AutoShares(Vec<String>),
    /// A set's single, freshly unsealed final share.
    FinalShare(String),
    /// The requested set name is not enrolled.
    UnknownSet,
    /// No sets are enrolled at all.
    Unenrolled,
    /// The supplied passphrase did not unseal the final share.
    BadPassphrase,
    /// A general error occurred while servicing the request.
    Error(String),
}
