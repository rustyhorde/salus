// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::Result;
use ssss::{SsssConfig, unlock};
use tracing::trace;

/// Generate a new key and split it into shares using Shamir's Secret Sharing Scheme.
///
/// # Errors
///
/// * If the random number generation fails, an error is returned.
/// * If the share generation fails, an error is returned.
///
pub fn gen_shares(config: &SsssConfig, key: &[u8; 32]) -> Result<Vec<String>> {
    trace!("Generating shares from key");
    ssss::gen_shares(config, key)
}

/// Unlock the key from the given shares using Shamir's Secret Sharing Scheme.
///
/// # Errors
///
///  * If the unlocking process fails, an error is returned.
///
pub fn unlock_key(shares: &[String]) -> Result<Vec<u8>> {
    unlock(shares)
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use aws_lc_rs::rand::fill;
    use rand::rng;
    use ssss::{SsssConfig, remove_random_entry};

    use super::{gen_shares, unlock_key};

    fn gen_key() -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        fill(&mut key)?;
        Ok(key)
    }

    #[test]
    fn gen_key_works() -> Result<()> {
        let key = gen_key()?;
        let shares = gen_shares(&SsssConfig::default(), &key)?;
        assert_eq!(shares.len(), 5);
        Ok(())
    }

    #[test]
    fn unlock_key_works() -> Result<()> {
        let key = gen_key()?;
        let mut shares = gen_shares(&SsssConfig::default(), &key)?;
        assert_eq!(shares.len(), 5);

        let unlocked = unlock_key(&shares)?;
        assert_eq!(unlocked.len(), 32);

        // Remove a random share from `shares` and check that 4 shares can unlock
        // the secret
        let mut rng = rng();
        remove_random_entry(&mut rng, &mut shares);
        assert_eq!(shares.len(), 4);
        assert_eq!(unlock_key(&shares)?, unlocked);

        // Remove a random share from `shares` and check that 3 shares can unlock
        // the secret
        remove_random_entry(&mut rng, &mut shares);
        assert_eq!(shares.len(), 3);
        assert_eq!(unlock_key(&shares)?, unlocked);

        // Remove a random share from `shares` and check that 2 shares *CANNOT* unlock
        // the secret
        remove_random_entry(&mut rng, &mut shares);
        assert_eq!(shares.len(), 2);
        assert_ne!(unlock_key(&shares)?, unlocked);

        Ok(())
    }
}
