// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::collections::HashMap;

use anyhow::Result;
use libsalus::SetInfo;
use tracing::{info, trace};
use zeroize::Zeroizing;

use crate::keystore;

/// One enrolled set, held in memory by the agent.
struct EnrolledSet {
    /// The `threshold - 1` automatic shares loaded from the keyring at startup.
    auto_shares: Vec<Zeroizing<String>>,
    /// The set's passphrase-sealed final-share blob (`salt || nonce || ct`).
    sealed_blob: Vec<u8>,
    /// The unsealed final share, cached after a successful unseal for the
    /// configured TTL so the passphrase is typed once per session.
    cached_final: Option<Zeroizing<String>>,
    /// Bumped each time the cache changes, so a stale clear timer is a no-op.
    cache_generation: u64,
}

/// The outcome of an unseal request.
pub(crate) enum UnsealResult {
    /// No set by that name is enrolled.
    Unknown,
    /// The passphrase did not unseal the final share.
    Bad,
    /// The final share, plus the generation to arm a cache-clear timer for when
    /// it was freshly cached.
    Share {
        /// The unsealed final share.
        value: String,
        /// `Some(generation)` when the share was freshly cached and a clear
        /// timer should be armed; `None` when it came from the cache or caching
        /// is disabled.
        arm_timer: Option<u64>,
    },
}

/// The agent's in-memory view of every enrolled set.
#[derive(Default)]
pub(crate) struct AgentState {
    sets: HashMap<String, EnrolledSet>,
}

impl AgentState {
    /// Load every enrolled set from the keyring into memory.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry or any automatic share cannot be read.
    pub(crate) fn load() -> Result<Self> {
        let mut sets = HashMap::new();
        for info in keystore::list_sets()? {
            let auto_shares = keystore::load_auto_shares(&info.name)?
                .into_iter()
                .map(Zeroizing::new)
                .collect();
            let Some(sealed_blob) = keystore::load_sealed_blob(&info.name)? else {
                trace!("set '{}' has no sealed final share; skipping", info.name);
                continue;
            };
            let _old = sets.insert(
                info.name.clone(),
                EnrolledSet {
                    auto_shares,
                    sealed_blob,
                    cached_final: None,
                    cache_generation: 0,
                },
            );
        }
        info!("loaded {} enrolled set(s)", sets.len());
        Ok(Self { sets })
    }

    /// Whether no sets are enrolled.
    pub(crate) fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// A wire-ready summary of every enrolled set.
    pub(crate) fn status(&self) -> Vec<SetInfo> {
        self.sets
            .iter()
            .map(|(name, set)| SetInfo {
                name: name.clone(),
                auto_count: u8::try_from(set.auto_shares.len()).unwrap_or(u8::MAX),
            })
            .collect()
    }

    /// A set's automatic shares, or `None` when the set is unknown.
    pub(crate) fn auto_shares(&self, name: &str) -> Option<Vec<String>> {
        self.sets.get(name).map(|set| {
            set.auto_shares
                .iter()
                .map(|share| share.as_str().to_string())
                .collect()
        })
    }

    /// Unseal a set's final share, caching it for the session when
    /// `cache_timeout > 0`.
    ///
    /// # Errors
    ///
    /// Returns an error only if the sealed blob is malformed; a wrong passphrase
    /// yields [`UnsealResult::Bad`].
    pub(crate) fn unseal(
        &mut self,
        name: &str,
        passphrase: &str,
        cache_timeout: u64,
    ) -> Result<UnsealResult> {
        let Some(set) = self.sets.get_mut(name) else {
            return Ok(UnsealResult::Unknown);
        };
        if let Some(cached) = &set.cached_final {
            return Ok(UnsealResult::Share {
                value: cached.as_str().to_string(),
                arm_timer: None,
            });
        }
        match keystore::unseal(&set.sealed_blob, passphrase)? {
            None => Ok(UnsealResult::Bad),
            Some(share) => {
                if cache_timeout > 0 {
                    set.cached_final = Some(Zeroizing::new(share.clone()));
                    set.cache_generation = set.cache_generation.wrapping_add(1);
                    Ok(UnsealResult::Share {
                        value: share,
                        arm_timer: Some(set.cache_generation),
                    })
                } else {
                    Ok(UnsealResult::Share {
                        value: share,
                        arm_timer: None,
                    })
                }
            }
        }
    }

    /// Clear a set's cached final share only if its cache generation matches.
    pub(crate) fn clear_cache_if_generation(&mut self, name: &str, generation: u64) {
        if let Some(set) = self.sets.get_mut(name)
            && set.cache_generation == generation
        {
            set.cached_final = None;
        }
    }

    /// Drop the cached final share for one set, or every set when `name` is
    /// `None`, bumping the generation so any pending clear timer is a no-op.
    pub(crate) fn lock(&mut self, name: Option<&str>) {
        match name {
            Some(name) => {
                if let Some(set) = self.sets.get_mut(name) {
                    set.cached_final = None;
                    set.cache_generation = set.cache_generation.wrapping_add(1);
                }
            }
            None => {
                for set in self.sets.values_mut() {
                    set.cached_final = None;
                    set.cache_generation = set.cache_generation.wrapping_add(1);
                }
            }
        }
    }
}
