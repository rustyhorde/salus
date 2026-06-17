// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! Local password and passphrase generation for the `gen` subcommand.
//!
//! Generation is performed entirely on the client with a cryptographically
//! secure RNG (`rand::rng()`); no key material or daemon round-trip is
//! involved unless the caller chooses to store the result.
//!
//! The bundled passphrase word list is the EFF "large" word list, which is
//! distributed by the Electronic Frontier Foundation under the Creative
//! Commons Attribution 3.0 United States license (CC BY 3.0 US). See
//! <https://www.eff.org/dice> and `eff_large_wordlist.txt`.

use anyhow::{Result, anyhow};
use crossterm::style::{Color, Stylize, style};
use rand::seq::{IndexedRandom, SliceRandom};
use zeroize::Zeroize;

use super::cli::GenKind;

/// Lowercase letters — always part of a generated password's alphabet.
const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
/// Uppercase letters, added when `caps` is enabled.
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// Decimal digits, added when `numbers` is enabled.
const DIGITS: &str = "0123456789";
/// Symbol characters, added when `special` is enabled.
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.<>?";

/// The EFF "large" word list (7776 words, one per line), embedded at build time.
const WORDLIST: &str = include_str!("eff_large_wordlist.txt");

/// Generate a password or passphrase.
///
/// When `passphrase` is `Some(n)` an `n`-word passphrase is produced using
/// `kind` for formatting and the character-class options are ignored.
/// Otherwise a `length`-character password is produced from lowercase letters
/// plus whichever of uppercase/digits/symbols are enabled, with at least one
/// character drawn from each enabled class.
pub(crate) fn generate(
    length: u32,
    caps: bool,
    numbers: bool,
    special: bool,
    passphrase: Option<u32>,
    kind: GenKind,
) -> Result<String> {
    match passphrase {
        Some(words) => gen_passphrase(words, kind),
        None => gen_password(length, caps, numbers, special),
    }
}

/// Build a random character password of `length` characters.
fn gen_password(length: u32, caps: bool, numbers: bool, special: bool) -> Result<String> {
    let mut rng = rand::rng();

    // The enabled character classes; lowercase is always present so the pool is
    // never empty.
    let mut classes: Vec<Vec<char>> = vec![LOWERCASE.chars().collect()];
    if caps {
        classes.push(UPPERCASE.chars().collect());
    }
    if numbers {
        classes.push(DIGITS.chars().collect());
    }
    if special {
        classes.push(SYMBOLS.chars().collect());
    }
    let pool: Vec<char> = classes.iter().flatten().copied().collect();

    let total = usize::try_from(length).unwrap_or(usize::MAX);
    let mut chars: Vec<char> = Vec::with_capacity(total);

    // Guarantee at least one character from every enabled class.
    for class in &classes {
        let chosen = class
            .choose(&mut rng)
            .copied()
            .ok_or_else(|| anyhow!("a character class was unexpectedly empty"))?;
        chars.push(chosen);
    }

    // Fill the remainder from the combined pool.
    let remaining = total.saturating_sub(chars.len());
    for _ in 0..remaining {
        let chosen = pool
            .choose(&mut rng)
            .copied()
            .ok_or_else(|| anyhow!("the character pool was unexpectedly empty"))?;
        chars.push(chosen);
    }

    // If `length` was somehow smaller than the number of guaranteed classes,
    // trim back to the requested size, then shuffle so the guaranteed
    // characters are not in predictable positions.
    chars.truncate(total);
    chars.shuffle(&mut rng);

    let password = chars.iter().collect::<String>();
    chars.zeroize();
    Ok(password)
}

/// Build a random passphrase of `words` words formatted according to `kind`.
fn gen_passphrase(words: u32, kind: GenKind) -> Result<String> {
    let list: Vec<&str> = WORDLIST.lines().filter(|line| !line.is_empty()).collect();
    let mut rng = rand::rng();

    let count = usize::try_from(words).unwrap_or(usize::MAX);
    let mut chosen: Vec<&str> = Vec::with_capacity(count);
    for _ in 0..count {
        let word = list
            .choose(&mut rng)
            .copied()
            .ok_or_else(|| anyhow!("the passphrase word list was unexpectedly empty"))?;
        chosen.push(word);
    }

    let separator = match kind {
        GenKind::Space => " ",
        GenKind::Hyphen => "-",
        GenKind::Dot => ".",
        GenKind::Camel => "",
    };
    let formatted: Vec<String> = chosen
        .iter()
        .map(|word| match kind {
            GenKind::Camel => capitalize(word),
            GenKind::Space | GenKind::Hyphen | GenKind::Dot => (*word).to_string(),
        })
        .collect();
    Ok(formatted.join(separator))
}

/// Return `word` with its first character upper-cased and the rest unchanged.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Print a generated secret with the same styling used for read values.
pub(crate) fn print_secret(secret: &str) {
    println!("{}", "Generated:".green());
    println!("{}", style(secret).with(Color::Green).bold());
}

#[cfg(test)]
mod test {
    use anyhow::Result;

    use super::{DIGITS, SYMBOLS, capitalize, gen_passphrase, gen_password};
    use crate::runtime::cli::GenKind;

    #[test]
    fn password_has_requested_length() -> Result<()> {
        let pw = gen_password(30, true, true, true)?;
        assert_eq!(pw.chars().count(), 30);
        Ok(())
    }

    #[test]
    fn password_honors_short_lengths() -> Result<()> {
        let pw = gen_password(8, true, true, true)?;
        assert_eq!(pw.chars().count(), 8);
        Ok(())
    }

    #[test]
    fn password_lowercase_only_when_classes_disabled() -> Result<()> {
        let pw = gen_password(64, false, false, false)?;
        assert!(
            pw.chars().all(|c| c.is_ascii_lowercase()),
            "expected only lowercase letters, got {pw}"
        );
        Ok(())
    }

    #[test]
    fn password_includes_each_enabled_class() -> Result<()> {
        let pw = gen_password(30, true, true, true)?;
        assert!(
            pw.chars().any(|c| c.is_ascii_lowercase()),
            "missing lowercase"
        );
        assert!(
            pw.chars().any(|c| c.is_ascii_uppercase()),
            "missing uppercase"
        );
        assert!(pw.chars().any(|c| DIGITS.contains(c)), "missing digit");
        assert!(pw.chars().any(|c| SYMBOLS.contains(c)), "missing symbol");
        Ok(())
    }

    #[test]
    fn passphrase_space_has_expected_word_count() -> Result<()> {
        let phrase = gen_passphrase(5, GenKind::Space)?;
        assert_eq!(phrase.split(' ').count(), 5);
        Ok(())
    }

    #[test]
    fn passphrase_dot_has_expected_word_count() -> Result<()> {
        let phrase = gen_passphrase(3, GenKind::Dot)?;
        assert_eq!(phrase.split('.').count(), 3);
        Ok(())
    }

    #[test]
    fn passphrase_camel_capitalizes_and_omits_separators() -> Result<()> {
        let phrase = gen_passphrase(4, GenKind::Camel)?;
        assert!(!phrase.contains(' '), "camel phrase should have no spaces");
        assert!(
            phrase.chars().next().is_some_and(char::is_uppercase),
            "camel phrase should start uppercase, got {phrase}"
        );
        Ok(())
    }

    #[test]
    fn capitalize_handles_words_and_empty() {
        assert_eq!(capitalize("horse"), "Horse");
        assert_eq!(capitalize(""), "");
    }
}
