// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Storage for enrolled share sets, shared by the client (which enrolls and
//! forgets sets) and the agent (which loads and unseals them).
//!
//! Each enrolled *set* needs `threshold` shares to unlock. `threshold - 1` of
//! them are stored automatically retrievable in the OS keyring; the final share
//! is argon2id + AES-256-GCM sealed behind a passphrase. The keyring is the
//! at-rest encryption and login gate for the automatic shares.
//!
//! ## Keyring layout (service `salus`)
//!
//! * `sets` — the [`Registry`]: the shared auto-share count plus one record per
//!   enrolled set.
//! * `auto-share-{i}` — the shared automatic shares reused by every
//!   non-independent set, so the keyring never holds `>= threshold` shares.
//! * `{set}/auto-share-{i}` — per-set automatic shares for `--independent-auto`
//!   sets (accepts the documented union risk).
//! * `{set}/final-blob` — the set's single passphrase-sealed share.

use anyhow::{Context, Result, anyhow, bail};
use argon2::Argon2;
use aws_lc_rs::{
    aead::{AES_256_GCM, Aad, Nonce, RandomizedNonceKey},
    rand,
};
use bincode_next::{Decode, Encode};
use keyring::{Entry, Error as KeyringError};
use libsalus::{SetInfo, decode, encode};
use zeroize::Zeroizing;

use crate::error::Error;

/// The keyring service all salus credentials live under.
const SERVICE: &str = "salus";
/// The keyring account holding the [`Registry`].
const REGISTRY_ACCOUNT: &str = "sets";
/// Length of the argon2id salt prepended to a sealed blob.
const SALT_LEN: usize = 16;
/// Length of the AES-256-GCM nonce stored in a sealed blob.
const NONCE_LEN: usize = 12;

/// The registry of enrolled sets, stored as a single keyring secret.
#[derive(Clone, Debug, Decode, Default, Encode)]
struct Registry {
    /// How many shared automatic shares exist (`auto-share-0..count`). Zero
    /// until the first non-independent set is enrolled.
    shared_auto_count: u8,
    /// One record per enrolled set.
    sets: Vec<SetRecord>,
}

/// A single enrolled set's metadata.
#[derive(Clone, Debug, Decode, Encode)]
struct SetRecord {
    name: String,
    auto_count: u8,
    independent_auto: bool,
}

fn entry(account: &str) -> Result<Entry> {
    Entry::new(SERVICE, account).with_context(|| format!("opening keyring entry '{account}'"))
}

fn read_registry() -> Result<Registry> {
    // `keyring::Error` is `#[non_exhaustive]`, so use `if let` for the one
    // variant we special-case (a missing registry) and let `?` propagate the
    // rest — a wildcard match arm would trip `non_exhaustive_omitted_patterns`.
    let secret = entry(REGISTRY_ACCOUNT)?.get_secret();
    if let Err(KeyringError::NoEntry) = &secret {
        return Ok(Registry::default());
    }
    decode::<Registry>(&secret?)
}

fn write_registry(registry: &Registry) -> Result<()> {
    let bytes = encode(registry.clone())?;
    entry(REGISTRY_ACCOUNT)?
        .set_secret(&bytes)
        .context("writing the keyring set registry")?;
    Ok(())
}

/// Derive a 32-byte key from `passphrase` and `salt` using argon2id.
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|e| anyhow!("argon2 key derivation failed: {e}"))?;
    Ok(key)
}

/// Seal a single share string behind `passphrase`.
///
/// Returns `salt || nonce || ciphertext`, suitable for keyring storage.
fn seal(plaintext: &str, passphrase: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    rand::fill(&mut salt)?;
    let key = derive_key(passphrase, &salt)?;
    let rnkey = RandomizedNonceKey::new(&AES_256_GCM, key.as_slice())
        .with_context(|| Error::NonceKeyGen)?;
    let mut in_out = plaintext.as_bytes().to_vec();
    let nonce = rnkey.seal_in_place_append_tag(Aad::empty(), &mut in_out)?;
    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + in_out.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(nonce.as_ref());
    blob.extend_from_slice(&in_out);
    Ok(blob)
}

/// Unseal a `salt || nonce || ciphertext` blob with `passphrase`.
///
/// Returns `Ok(None)` when the passphrase is wrong (authentication fails) and an
/// `Err` only for a malformed blob or a key-derivation failure.
///
/// # Errors
///
/// Returns an error if the blob is too short, key derivation fails, or the
/// decrypted bytes are not valid UTF-8.
pub fn unseal(blob: &[u8], passphrase: &str) -> Result<Option<String>> {
    if blob.len() < SALT_LEN + NONCE_LEN {
        bail!("sealed share blob is malformed (too short)");
    }
    let (salt, rest) = blob.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);
    let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_e| anyhow!("invalid nonce length"))?;
    let key = derive_key(passphrase, salt)?;
    let rnkey = RandomizedNonceKey::new(&AES_256_GCM, key.as_slice())
        .with_context(|| Error::NonceKeyGen)?;
    let mut buf = ciphertext.to_vec();
    match rnkey.open_in_place(Nonce::from(&nonce_arr), Aad::empty(), &mut buf) {
        Ok(plaintext) => {
            let share = String::from_utf8(plaintext.to_vec())
                .context("decrypted share is not valid UTF-8")?;
            Ok(Some(share))
        }
        // A failed open means the passphrase (and therefore the derived key) is
        // wrong. That is a normal "bad passphrase", not a hard error.
        Err(_e) => Ok(None),
    }
}

/// The count of shared automatic shares, if any have been established.
///
/// `Some(n)` means a non-independent set has already fixed the shared
/// `auto-share-*` entries, so a later set need only supply its final share.
///
/// # Errors
///
/// Returns an error if the registry cannot be read.
pub fn shared_auto_count() -> Result<Option<u8>> {
    let registry = read_registry()?;
    Ok((registry.shared_auto_count > 0).then_some(registry.shared_auto_count))
}

/// Enroll a set from a full slice of `threshold` shares.
///
/// The first `threshold - 1` shares become the automatic shares (shared, or
/// per-set when `independent`) and the final share is sealed behind
/// `passphrase`.
///
/// # Errors
///
/// Returns an error if fewer than two shares are supplied, the set already
/// exists and `force` is false, or any keyring/crypto operation fails.
pub fn enroll_full(
    name: &str,
    shares: &[String],
    passphrase: &str,
    independent: bool,
    force: bool,
) -> Result<()> {
    if shares.len() < 2 {
        bail!("enrollment needs at least the threshold number of shares (>= 2)");
    }
    let auto_count = u8::try_from(shares.len() - 1).context("too many shares to enroll")?;
    let mut registry = read_registry()?;
    if registry.sets.iter().any(|r| r.name == name) && !force {
        bail!("a set named '{name}' is already enrolled; pass --force to replace it");
    }

    let (auto, manual) = shares.split_at(shares.len() - 1);
    let final_share = &manual[0];

    if independent {
        for (i, share) in auto.iter().enumerate() {
            entry(&format!("{name}/auto-share-{i}"))?
                .set_password(share)
                .context("writing a per-set automatic share")?;
        }
    } else {
        for (i, share) in auto.iter().enumerate() {
            entry(&format!("auto-share-{i}"))?
                .set_password(share)
                .context("writing a shared automatic share")?;
        }
        registry.shared_auto_count = auto_count;
    }

    let blob = seal(final_share, passphrase)?;
    entry(&format!("{name}/final-blob"))?
        .set_secret(&blob)
        .context("writing the sealed final share")?;

    registry.sets.retain(|r| r.name != name);
    registry.sets.push(SetRecord {
        name: name.to_string(),
        auto_count,
        independent_auto: independent,
    });
    write_registry(&registry)?;
    Ok(())
}

/// Enroll a set that reuses the already-established shared automatic shares,
/// supplying only its single passphrase-sealed final share.
///
/// # Errors
///
/// Returns an error if no shared automatic shares exist yet, the set already
/// exists and `force` is false, or any keyring/crypto operation fails.
pub fn enroll_final_only(
    name: &str,
    final_share: &str,
    passphrase: &str,
    force: bool,
) -> Result<()> {
    let mut registry = read_registry()?;
    let shared_auto_count = registry.shared_auto_count;
    if shared_auto_count == 0 {
        bail!("no shared automatic shares exist yet; enroll a first set fully");
    }
    if registry.sets.iter().any(|r| r.name == name) && !force {
        bail!("a set named '{name}' is already enrolled; pass --force to replace it");
    }

    let blob = seal(final_share, passphrase)?;
    entry(&format!("{name}/final-blob"))?
        .set_secret(&blob)
        .context("writing the sealed final share")?;

    registry.sets.retain(|r| r.name != name);
    registry.sets.push(SetRecord {
        name: name.to_string(),
        auto_count: shared_auto_count,
        independent_auto: false,
    });
    write_registry(&registry)?;
    Ok(())
}

/// Remove a single enrolled set, returning whether it existed.
///
/// Shared automatic shares are only removed once the last set is forgotten.
///
/// # Errors
///
/// Returns an error if the registry cannot be read or rewritten.
pub fn forget(name: &str) -> Result<bool> {
    let mut registry = read_registry()?;
    let Some(pos) = registry.sets.iter().position(|r| r.name == name) else {
        return Ok(false);
    };
    let record = registry.sets.remove(pos);

    let _del = entry(&format!("{name}/final-blob"))?.delete_credential();
    if record.independent_auto {
        for i in 0..record.auto_count {
            let _del = entry(&format!("{name}/auto-share-{i}"))?.delete_credential();
        }
    }

    if registry.sets.is_empty() {
        for i in 0..registry.shared_auto_count {
            let _del = entry(&format!("auto-share-{i}"))?.delete_credential();
        }
        let _del = entry(REGISTRY_ACCOUNT)?.delete_credential();
    } else {
        write_registry(&registry)?;
    }
    Ok(true)
}

/// Remove every enrolled set and the shared automatic shares.
///
/// # Errors
///
/// Returns an error if the registry cannot be read.
pub fn forget_all() -> Result<()> {
    let registry = read_registry()?;
    for record in &registry.sets {
        let _del = entry(&format!("{}/final-blob", record.name))?.delete_credential();
        if record.independent_auto {
            for i in 0..record.auto_count {
                let _del = entry(&format!("{}/auto-share-{i}", record.name))?.delete_credential();
            }
        }
    }
    for i in 0..registry.shared_auto_count {
        let _del = entry(&format!("auto-share-{i}"))?.delete_credential();
    }
    let _del = entry(REGISTRY_ACCOUNT)?.delete_credential();
    Ok(())
}

/// List every enrolled set as a wire-ready [`SetInfo`].
///
/// # Errors
///
/// Returns an error if the registry cannot be read.
pub fn list_sets() -> Result<Vec<SetInfo>> {
    let registry = read_registry()?;
    Ok(registry
        .sets
        .iter()
        .map(|r| SetInfo {
            name: r.name.clone(),
            auto_count: r.auto_count,
        })
        .collect())
}

/// Load a set's automatic shares from the keyring (used by the agent at start).
///
/// # Errors
///
/// Returns an error if the set is unknown or a keyring read fails.
pub fn load_auto_shares(name: &str) -> Result<Vec<String>> {
    let registry = read_registry()?;
    let Some(record) = registry.sets.iter().find(|r| r.name == name) else {
        bail!("no enrolled set named '{name}'");
    };
    let mut shares = Vec::with_capacity(usize::from(record.auto_count));
    for i in 0..record.auto_count {
        let account = if record.independent_auto {
            format!("{name}/auto-share-{i}")
        } else {
            format!("auto-share-{i}")
        };
        shares.push(
            entry(&account)?
                .get_password()
                .with_context(|| format!("reading automatic share '{account}'"))?,
        );
    }
    Ok(shares)
}

/// Load a set's sealed final-share blob from the keyring.
///
/// # Errors
///
/// Returns an error if a keyring read fails for a reason other than the entry
/// being absent.
pub fn load_sealed_blob(name: &str) -> Result<Option<Vec<u8>>> {
    // See `read_registry` for why this uses `if let` rather than a match with a
    // wildcard arm over the `#[non_exhaustive]` `keyring::Error`.
    let secret = entry(&format!("{name}/final-blob"))?.get_secret();
    if let Err(KeyringError::NoEntry) = &secret {
        return Ok(None);
    }
    Ok(Some(secret?))
}

#[cfg(test)]
mod test {
    use super::{seal, unseal};

    #[test]
    fn seal_unseal_round_trip() {
        let blob = seal("share-value", "correct horse battery staple").unwrap();
        let out = unseal(&blob, "correct horse battery staple").unwrap();
        assert_eq!(out.as_deref(), Some("share-value"));
    }

    #[test]
    fn wrong_passphrase_returns_none() {
        let blob = seal("share-value", "right-passphrase").unwrap();
        let out = unseal(&blob, "wrong-passphrase").unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn malformed_blob_errors() {
        assert!(unseal(b"too-short", "whatever").is_err());
    }
}
