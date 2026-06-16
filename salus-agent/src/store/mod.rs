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

#[cfg(test)]
mod test {
    use anyhow::{Context, Result, bail};
    use zeroize::Zeroizing;

    use super::{AgentState, EnrolledSet, UnsealResult};
    use crate::{keystore, test_keyring::guard};

    fn enrolled(auto: &[&str], cached: Option<&str>) -> EnrolledSet {
        EnrolledSet {
            auto_shares: auto
                .iter()
                .map(|s| Zeroizing::new((*s).to_string()))
                .collect(),
            sealed_blob: Vec::new(),
            cached_final: cached.map(|s| Zeroizing::new(s.to_string())),
            cache_generation: 0,
        }
    }

    fn state(sets: Vec<(&str, EnrolledSet)>) -> AgentState {
        AgentState {
            sets: sets.into_iter().map(|(n, e)| (n.to_string(), e)).collect(),
        }
    }

    #[test]
    fn is_empty_reflects_sets() {
        assert!(state(vec![]).is_empty());
        assert!(!state(vec![("a", enrolled(&["s0"], None))]).is_empty());
    }

    #[test]
    fn status_reports_auto_count() -> Result<()> {
        let st = state(vec![("a", enrolled(&["s0", "s1"], None))]);
        let status = st.status();
        assert_eq!(status.len(), 1);
        let first = status.first().context("expected one status entry")?;
        assert_eq!(first.name, "a");
        assert_eq!(first.auto_count, 2);
        Ok(())
    }

    #[test]
    fn auto_shares_known_and_unknown() {
        let st = state(vec![("a", enrolled(&["s0", "s1"], None))]);
        assert_eq!(
            st.auto_shares("a"),
            Some(vec!["s0".to_string(), "s1".to_string()])
        );
        assert!(st.auto_shares("missing").is_none());
    }

    #[test]
    fn unseal_unknown_set() -> Result<()> {
        let mut st = state(vec![]);
        assert!(matches!(st.unseal("a", "p", 0)?, UnsealResult::Unknown));
        Ok(())
    }

    #[test]
    fn unseal_returns_cached_without_arming_timer() -> Result<()> {
        let mut st = state(vec![("a", enrolled(&["s0"], Some("cached-share")))]);
        match st.unseal("a", "ignored", 3600)? {
            UnsealResult::Share { value, arm_timer } => {
                assert_eq!(value, "cached-share");
                assert!(arm_timer.is_none());
            }
            _ => bail!("expected a cached share result"),
        }
        Ok(())
    }

    #[test]
    fn clear_cache_only_on_matching_generation() -> Result<()> {
        let mut st = state(vec![("a", enrolled(&["s0"], Some("cached")))]);
        st.clear_cache_if_generation("a", 99);
        assert!(st.sets.get("a").context("set a")?.cached_final.is_some());
        st.clear_cache_if_generation("a", 0);
        assert!(st.sets.get("a").context("set a")?.cached_final.is_none());
        Ok(())
    }

    #[test]
    fn lock_one_clears_cache_and_bumps_generation() -> Result<()> {
        let mut st = state(vec![
            ("a", enrolled(&["s0"], Some("cached"))),
            ("b", enrolled(&["s0"], Some("cached"))),
        ]);
        st.lock(Some("a"));
        let a = st.sets.get("a").context("set a")?;
        assert!(a.cached_final.is_none());
        assert_eq!(a.cache_generation, 1);
        // The other set is untouched.
        let b = st.sets.get("b").context("set b")?;
        assert!(b.cached_final.is_some());
        assert_eq!(b.cache_generation, 0);
        Ok(())
    }

    #[test]
    fn lock_all_clears_every_cache() -> Result<()> {
        let mut st = state(vec![
            ("a", enrolled(&["s0"], Some("cached"))),
            ("b", enrolled(&["s0"], Some("cached"))),
        ]);
        st.lock(None);
        assert!(st.sets.get("a").context("set a")?.cached_final.is_none());
        assert!(st.sets.get("b").context("set b")?.cached_final.is_none());
        Ok(())
    }

    #[test]
    fn load_populates_state_and_unseal_caches() -> Result<()> {
        let _g = guard();
        keystore::enroll_full(
            "alpha",
            &["s0".into(), "s1".into(), "final".into()],
            "pass",
            false,
            false,
        )?;

        let mut st = AgentState::load()?;
        assert_eq!(st.status().len(), 1);
        assert_eq!(
            st.auto_shares("alpha"),
            Some(vec!["s0".into(), "s1".into()])
        );

        // Wrong passphrase is a soft failure.
        assert!(matches!(st.unseal("alpha", "wrong", 0)?, UnsealResult::Bad));

        // A fresh correct unseal with caching enabled arms a clear timer.
        match st.unseal("alpha", "pass", 3600)? {
            UnsealResult::Share { value, arm_timer } => {
                assert_eq!(value, "final");
                assert_eq!(arm_timer, Some(1));
            }
            _ => bail!("expected a fresh share result"),
        }
        // The next unseal is served from the cache, so it does not re-arm.
        match st.unseal("alpha", "pass", 3600)? {
            UnsealResult::Share { arm_timer, .. } => assert!(arm_timer.is_none()),
            _ => bail!("expected a cached share result"),
        }
        Ok(())
    }

    #[test]
    fn unseal_without_caching_does_not_arm_timer() -> Result<()> {
        let _g = guard();
        keystore::enroll_full(
            "beta",
            &["s0".into(), "s1".into(), "final".into()],
            "pass",
            false,
            false,
        )?;
        let mut st = AgentState::load()?;
        match st.unseal("beta", "pass", 0)? {
            UnsealResult::Share { value, arm_timer } => {
                assert_eq!(value, "final");
                assert!(arm_timer.is_none());
            }
            _ => bail!("expected a share result"),
        }
        Ok(())
    }
}
