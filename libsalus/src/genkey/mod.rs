// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::Result;
use aws_lc_rs::rand;
use ssss::{SsssConfig, gen_shares};

/// Generate a new key and split it into shares using Shamir's Secret Sharing Scheme.
///
/// # Errors
///
/// * If the random number generation fails, an error is returned.
/// * If the share generation fails, an error is returned.
///
pub fn genkey(config: &SsssConfig) -> Result<Vec<String>> {
    let mut rand_bytes = [0u8; 256];
    rand::fill(&mut rand_bytes)?;

    let shares = gen_shares(config, &rand_bytes)?;

    rand_bytes.fill(0);
    Ok(shares)
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use ssss::SsssConfig;

    use super::genkey;

    #[test]
    fn genkey_works() -> Result<()> {
        let shares = genkey(&SsssConfig::default())?;
        assert_eq!(shares.len(), 5);
        for (i, share) in shares.iter().enumerate() {
            eprintln!("Share {}: {}", i + 1, share);
        }
        Ok(())
    }
}
