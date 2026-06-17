// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::io::{IsTerminal as _, Write, stdin, stdout};

use anyhow::{Context, Result, bail};
use bon::Builder;
use crossterm::style::{Color, Stylize, style};
use interprocess::local_socket::{tokio::Stream, traits::tokio::Stream as _};
use libsalus::{
    Action, AgentAction, AgentResponse, MAX_UNLOCK_SECONDS, Response, SetInfo, Share, Store,
    UnlockTimeout, agent_socket_name, decode, encode, socket_name,
};
use salus_agent::keystore;
use scanpw::scanpw;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Builder, Clone, Debug)]
pub(crate) struct Inter {
    /// Optional override for the daemon IPC socket path. When `None`, libsalus
    /// resolves the shared `SALUS_SOCKET` env var or the platform default.
    #[builder(into)]
    name: Option<String>,
    /// Optional override for the `salus-agent` IPC socket path. When `None`,
    /// libsalus resolves `SALUS_AGENT_SOCKET` or the platform default.
    #[builder(into)]
    agent_name: Option<String>,
}

impl Inter {
    pub(crate) async fn send(&self, message: Action) -> Result<Response> {
        // Resolve the socket name, honoring any configured override.
        let name = socket_name(self.name.as_deref())?;

        // Await this here since we can't do a whole lot without a connection.
        let conn = Stream::connect(name).await?;

        // This consumes our connection and splits it into two halves, so that we can concurrently use
        // both.
        let (recver, mut sender) = conn.split();
        let mut recver = BufReader::new(recver);

        // Describe the send operation as writing our whole string.
        let _handle = tokio::spawn(async move {
            let blah = async || -> Result<()> {
                let message = encode(message)?;
                sender.write_all(&message).await?;
                sender.flush().await?;
                Ok(())
            };
            if let Err(e) = blah().await {
                eprintln!("There was an error when sending: {e}");
            }
            drop(sender);
        });

        // Describe the receive operation as receiving until a newline into our buffer.
        let mut msg_buf = Vec::new();
        let _msg_size = recver.read_to_end(&mut msg_buf).await?;
        decode::<Response>(&msg_buf)
    }

    /// Send a single `AgentAction` to the `salus-agent` and read its response.
    ///
    /// Mirrors [`Inter::send`] but speaks the agent protocol over the agent
    /// socket. An error here typically means the agent is not running, which the
    /// unlock flow treats as "fall back to manual share entry".
    async fn agent_send(&self, message: AgentAction) -> Result<AgentResponse> {
        let name = agent_socket_name(self.agent_name.as_deref())?;
        let conn = Stream::connect(name).await?;
        let (recver, mut sender) = conn.split();
        let mut recver = BufReader::new(recver);

        let _handle = tokio::spawn(async move {
            let send = async || -> Result<()> {
                let message = encode(message)?;
                sender.write_all(&message).await?;
                sender.flush().await?;
                Ok(())
            };
            if let Err(e) = send().await {
                eprintln!("There was an error when sending to the agent: {e}");
            }
            drop(sender);
        });

        let mut msg_buf = Vec::new();
        let _msg_size = recver.read_to_end(&mut msg_buf).await?;
        decode::<AgentResponse>(&msg_buf)
    }

    pub(crate) async fn shares(&self, num_shares: u8, threshold: u8) -> Result<()> {
        match self.send(Action::GenShares(num_shares, threshold)).await? {
            Response::Shares(shares) => {
                println!("{}", "These are your salus key shares.  Record them somewhere safe!  They will not be shown again.".green().bold());
                println!();
                for share in shares.shares() {
                    println!("{share}");
                }
            }
            Response::AlreadyInitialiazed => {
                println!(
                    "{}",
                    "The shares for this salus store have already been generated"
                        .red()
                        .bold()
                );
            }
            Response::Error(error) => {
                eprintln!("Error occurred while generating shares: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }

    pub(crate) async fn unlock(&self, set: Option<String>, duration: Option<String>) -> Result<()> {
        let timeout = parse_unlock_timeout(duration.as_deref());

        let mut threshold = 3;
        if let Response::Threshold(th) = self.send(Action::GetThreshold).await? {
            threshold = th;
        }

        // Prefer the agent: it serves the threshold-1 auto shares and unseals the
        // final share with a single passphrase. On any failure (agent absent,
        // unenrolled, unknown set, bad passphrase) fall back to manual entry.
        let supplied = match self.collect_shares_via_agent(set).await {
            Ok(Some(shares)) => {
                for share in shares {
                    let message = Action::Share(Share::builder().share(share).build());
                    let _unused = self.send(message).await?;
                }
                true
            }
            Ok(None) => false,
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Agent unavailable ({e}); entering shares manually").yellow()
                );
                false
            }
        };

        if !supplied {
            let th_prompt = format!("Enter your {threshold} shares, one per prompt");
            println!("{}", th_prompt.green().bold());
            println!();
            for i in 0..threshold {
                let share_in = scanpw!(
                    "{}",
                    style(format!("Enter share {}/{threshold}: ", i.saturating_add(1))).green()
                );
                let share = Share::builder().share(share_in).build();
                let message = Action::Share(share);
                let _unused = self.send(message).await?;
            }
        }

        match self.send(Action::Unlock(timeout)).await? {
            Response::Success => {
                println!("{}", "Store unlocked".green().bold());
            }
            Response::UnlockFailed => {
                eprintln!(
                    "{}",
                    "Unlock failed: the provided shares did not reconstruct the key"
                        .red()
                        .bold()
                );
            }
            Response::Error(error) => {
                eprintln!("Error occurred while unlocking: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }

    /// Try to gather the unlock shares from the `salus-agent`.
    ///
    /// Returns `Ok(Some(shares))` when the agent supplied a full set (the
    /// `threshold - 1` auto shares plus the unsealed final share), `Ok(None)`
    /// when the caller should fall back to manual entry (agent unreachable,
    /// nothing enrolled, unknown set, or a bad passphrase), and `Err` only on a
    /// protocol error worth surfacing.
    async fn collect_shares_via_agent(&self, set: Option<String>) -> Result<Option<Vec<String>>> {
        // Probe the agent. A connection error means it is not running.
        let Ok(AgentResponse::Status { sets }) = self.agent_send(AgentAction::Status).await else {
            return Ok(None);
        };
        if sets.is_empty() {
            return Ok(None);
        }

        let set_name = match set {
            Some(name) => name,
            None => match sets.first() {
                Some(only) if sets.len() == 1 => only.name.clone(),
                _ => choose_set(&sets)?,
            },
        };

        let mut shares = match self
            .agent_send(AgentAction::GetAutoShares {
                set: set_name.clone(),
            })
            .await?
        {
            AgentResponse::AutoShares(shares) => shares,
            AgentResponse::UnknownSet => {
                eprintln!(
                    "{}",
                    format!("No enrolled set named '{set_name}'; entering shares manually")
                        .yellow()
                );
                return Ok(None);
            }
            _ => return Ok(None),
        };

        let passphrase = scanpw!(
            "{}",
            style(format!("Enter passphrase for set '{set_name}': ")).green()
        );
        match self
            .agent_send(AgentAction::UnsealFinal {
                set: set_name,
                passphrase,
            })
            .await?
        {
            AgentResponse::FinalShare(share) => {
                shares.push(share);
                Ok(Some(shares))
            }
            AgentResponse::BadPassphrase => {
                eprintln!(
                    "{}",
                    "Incorrect passphrase; entering shares manually".yellow()
                );
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    pub(crate) async fn lock(&self) -> Result<()> {
        match self.send(Action::Lock).await? {
            Response::Success => {
                println!("{}", "Store locked".green().bold());
            }
            Response::Error(error) => {
                eprintln!("Error occurred while locking: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }

    pub(crate) async fn enroll(
        &self,
        name: String,
        force: bool,
        independent_auto: bool,
    ) -> Result<()> {
        // Reuse the established shared automatic shares when they exist and this
        // is not an independent enrollment: only the one new final share and its
        // passphrase are needed.
        let reuse = !independent_auto && keystore::shared_auto_count()?.is_some();

        if reuse {
            println!(
                "{}",
                format!("Reusing the shared automatic shares for set '{name}'.").green()
            );
            let share = scanpw!(
                "{}",
                style(format!(
                    "Enter the passphrase-protected share for set '{name}': "
                ))
                .green()
            );
            let passphrase = prompt_passphrase_confirm()?;
            keystore::enroll_final_only(&name, &share, &passphrase, force)?;
        } else {
            let threshold = self.threshold_or_prompt().await?;
            if threshold < 2 {
                bail!("a threshold of at least 2 is required to enroll");
            }
            println!(
                "{}",
                format!("Enter the {threshold} shares for set '{name}', one per prompt.")
                    .green()
                    .bold()
            );
            let mut shares = Vec::with_capacity(usize::from(threshold));
            for i in 0..threshold {
                let share = scanpw!(
                    "{}",
                    style(format!("Enter share {}/{threshold}: ", i.saturating_add(1))).green()
                );
                shares.push(share);
            }
            let passphrase = prompt_passphrase_confirm()?;
            keystore::enroll_full(&name, &shares, &passphrase, independent_auto, force)?;
        }

        println!("{}", format!("Enrolled set '{name}'.").green().bold());
        Ok(())
    }

    pub(crate) async fn enroll_status(&self) -> Result<()> {
        let sets = keystore::list_sets()?;
        if sets.is_empty() {
            println!("{}", "No sets are enrolled.".yellow());
        } else {
            println!("{}", "Enrolled sets:".green().bold());
            for info in &sets {
                println!(
                    "  {} ({} automatic share(s) + 1 passphrase)",
                    info.name, info.auto_count
                );
            }
        }
        match self.agent_send(AgentAction::Status).await {
            Ok(_) => println!("{}", "Agent: reachable".green()),
            Err(_) => println!("{}", "Agent: not reachable".red()),
        }
        Ok(())
    }

    /// Ask the daemon for the configured threshold, falling back to a prompt when
    /// the daemon is unreachable.
    async fn threshold_or_prompt(&self) -> Result<u8> {
        if let Ok(Response::Threshold(threshold)) = self.send(Action::GetThreshold).await {
            return Ok(threshold);
        }
        let line = prompt_line("How many shares does unlocking require? [3]: ")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            Ok(3)
        } else {
            trimmed
                .parse::<u8>()
                .context("the threshold must be a small whole number")
        }
    }

    pub(crate) async fn store(&self, key: String, value: String, force: bool) -> Result<()> {
        let message = Action::Store(
            Store::builder()
                .key(key.clone())
                .value(value.clone())
                .force(force)
                .build(),
        );
        match self.send(message).await? {
            Response::Success => {}
            Response::KeyExists => {
                // The key already exists. Confirm before overwriting; when stdin
                // is not a terminal we cannot prompt, so a non-interactive
                // overwrite must pass `--force` rather than be silently confirmed
                // by piped input.
                if !stdin().is_terminal() {
                    eprintln!(
                        "{}",
                        format!(
                            "Refusing to overwrite existing key '{key}' without confirmation; \
                             re-run with --force to overwrite"
                        )
                        .red()
                        .bold()
                    );
                    return Ok(());
                }
                let answer = prompt_line(&format!("Overwrite key '{key}'? [y/N]: "))?;
                let answer = answer.trim().to_ascii_lowercase();
                if answer != "y" && answer != "yes" {
                    println!("{}", "Aborted; nothing was stored.".yellow());
                    return Ok(());
                }
                let forced =
                    Action::Store(Store::builder().key(key).value(value).force(true).build());
                if let Response::Error(error) = self.send(forced).await? {
                    eprintln!("Error occurred while storing value: {error}");
                }
            }
            Response::Error(error) => {
                eprintln!("Error occurred while storing value: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }

    pub(crate) async fn read(&self, key: String) -> Result<()> {
        let message = Action::Read(key.clone());
        match self.send(message).await? {
            Response::Value(value) => {
                if let Some(bytes) = value {
                    match String::from_utf8(bytes) {
                        Ok(val) => {
                            let value_style = style(val).with(Color::Green).bold();
                            println!("{}", "Value: ".green());
                            println!("{value_style}");
                        }
                        Err(e) => {
                            let len = e.as_bytes().len();
                            let binary_style = style(format!(
                                "Value for '{key}' is {len} bytes of non-UTF-8 binary data"
                            ))
                            .with(Color::Yellow)
                            .bold();
                            println!("{binary_style}");
                        }
                    }
                } else {
                    let not_found_style = style(format!("No value found for '{key}'")).red().bold();
                    println!("{not_found_style}");
                }
            }
            Response::KeyNotFound => {
                let not_found_style = style(format!("Key '{key}' not found")).red().bold();
                println!("{not_found_style}");
            }
            Response::Error(error) => {
                eprintln!("Error occurred while reading value: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }

    pub(crate) async fn delete(&self, key: String, force: bool) -> Result<()> {
        // Confirm by default. A destructive delete should never proceed without
        // an explicit yes: when stdin is not a terminal we cannot prompt, so a
        // non-interactive delete must pass `--force` rather than be silently
        // confirmed by piped input.
        if !force {
            if !stdin().is_terminal() {
                eprintln!(
                    "{}",
                    format!(
                        "Refusing to delete '{key}' without confirmation; \
                         re-run with --force for non-interactive deletes"
                    )
                    .red()
                    .bold()
                );
                return Ok(());
            }
            let answer = prompt_line(&format!("Delete key '{key}'? [y/N]: "))?;
            let answer = answer.trim().to_ascii_lowercase();
            if answer != "y" && answer != "yes" {
                println!("{}", "Aborted; nothing was deleted.".yellow());
                return Ok(());
            }
        }

        match self.send(Action::Delete(key.clone())).await? {
            Response::Success => {
                println!("{}", format!("Removed key '{key}'.").green().bold());
            }
            Response::KeyNotFound => {
                let not_found_style = style(format!("Key '{key}' not found")).red().bold();
                println!("{not_found_style}");
            }
            Response::Error(error) => {
                eprintln!("Error occurred while deleting value: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }

    pub(crate) async fn find(&self, regex: String) -> Result<()> {
        let message = Action::FindKey(regex.clone());
        match self.send(message).await? {
            Response::Matches(matches) => {
                if matches.is_empty() {
                    let no_match_style = style(format!("No keys matched regex '{regex}'"))
                        .red()
                        .bold();
                    println!("{no_match_style}");
                } else {
                    println!("{}", "Matching keys:".green().bold());
                    for key in matches {
                        let key_style = style(key).with(Color::Green).bold();
                        println!("{key_style}");
                    }
                }
            }
            Response::Error(error) => {
                eprintln!("Error occurred while finding key: {error}");
            }
            _ => {
                eprintln!("Unexpected response from salusd");
            }
        }
        Ok(())
    }
}

/// Remove a named enrolled set, or every set when `all` is set.
///
/// # Errors
///
/// Returns an error if neither a name nor `--all` is given, or a keyring
/// operation fails.
pub(crate) fn forget(name: Option<&str>, all: bool) -> Result<()> {
    if all {
        keystore::forget_all()?;
        println!("{}", "Removed all enrolled sets.".green().bold());
    } else if let Some(name) = name {
        if keystore::forget(name)? {
            println!(
                "{}",
                format!("Removed enrolled set '{name}'.").green().bold()
            );
        } else {
            eprintln!("{}", format!("No enrolled set named '{name}'.").yellow());
        }
    } else {
        bail!("specify --name <set> to remove one set, or --all to remove every set");
    }
    Ok(())
}

/// Prompt twice (no echo) for a passphrase and confirm they match.
fn prompt_passphrase_confirm() -> Result<String> {
    loop {
        let first = scanpw!(
            "{}",
            style("Enter a passphrase to protect the final share: ").green()
        );
        if first.is_empty() {
            eprintln!("{}", "Passphrase cannot be empty".red());
            continue;
        }
        let second = scanpw!("{}", style("Confirm passphrase: ").green());
        if first == second {
            return Ok(first);
        }
        eprintln!("{}", "Passphrases did not match, try again".red());
    }
}

/// Print `prompt` and read a line of (echoed) input from stdin.
fn prompt_line(prompt: &str) -> Result<String> {
    print!("{}", prompt.green());
    stdout().flush()?;
    let mut line = String::new();
    let _read = stdin().read_line(&mut line)?;
    Ok(line)
}

/// Parse the `--for` duration into an [`UnlockTimeout`].
///
/// Accepts a plain number of seconds (clamped to [`MAX_UNLOCK_SECONDS`], with a
/// warning when it exceeds the cap) or `forever`/`inf`/`infinite`. An empty or
/// unparseable value falls back to the daemon's configured default.
fn parse_unlock_timeout(duration: Option<&str>) -> UnlockTimeout {
    let Some(duration) = duration else {
        return UnlockTimeout::Default;
    };
    let duration = duration.trim();
    if duration.eq_ignore_ascii_case("forever")
        || duration.eq_ignore_ascii_case("inf")
        || duration.eq_ignore_ascii_case("infinite")
    {
        return UnlockTimeout::Forever;
    }
    match duration.parse::<u64>() {
        Ok(secs) if secs > MAX_UNLOCK_SECONDS => {
            eprintln!(
                "{}",
                format!(
                    "Requested {secs}s exceeds the 24h maximum; capping at {MAX_UNLOCK_SECONDS}s"
                )
                .yellow()
            );
            UnlockTimeout::Seconds(MAX_UNLOCK_SECONDS)
        }
        Ok(secs) => UnlockTimeout::Seconds(secs),
        Err(_) => {
            eprintln!(
                "{}",
                format!("Could not parse duration '{duration}'; using the daemon default").yellow()
            );
            UnlockTimeout::Default
        }
    }
}

/// Prompt the user to pick one of several enrolled sets by number.
fn choose_set(sets: &[SetInfo]) -> Result<String> {
    println!("{}", "Multiple enrolled sets are available:".green().bold());
    for (idx, info) in sets.iter().enumerate() {
        println!("  {}) {}", idx.saturating_add(1), info.name);
    }
    loop {
        print!("Select a set [1-{}]: ", sets.len());
        stdout().flush()?;
        let mut line = String::new();
        let _read = stdin().read_line(&mut line)?;
        match parse_set_choice(&line, sets.len()).and_then(|idx| sets.get(idx)) {
            Some(info) => return Ok(info.name.clone()),
            None => eprintln!("{}", "Invalid selection, try again".red()),
        }
    }
}

/// Parse a 1-based set selection into a 0-based index, or `None` when the input
/// is not a whole number in `1..=len`.
fn parse_set_choice(line: &str, len: usize) -> Option<usize> {
    match line.trim().parse::<usize>() {
        Ok(choice) if choice >= 1 && choice <= len => Some(choice.saturating_sub(1)),
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use libsalus::{MAX_UNLOCK_SECONDS, UnlockTimeout};

    use super::{parse_set_choice, parse_unlock_timeout};

    #[test]
    fn set_choice_valid_is_zero_based() {
        assert_eq!(parse_set_choice("1\n", 3), Some(0));
        assert_eq!(parse_set_choice("  3 ", 3), Some(2));
    }

    #[test]
    fn set_choice_out_of_range_is_none() {
        assert_eq!(parse_set_choice("0", 3), None);
        assert_eq!(parse_set_choice("4", 3), None);
    }

    #[test]
    fn set_choice_non_numeric_is_none() {
        assert_eq!(parse_set_choice("abc", 3), None);
        assert_eq!(parse_set_choice("", 3), None);
    }

    #[test]
    fn none_is_default() {
        assert_eq!(parse_unlock_timeout(None), UnlockTimeout::Default);
    }

    #[test]
    fn forever_variants() {
        assert_eq!(
            parse_unlock_timeout(Some("forever")),
            UnlockTimeout::Forever
        );
        assert_eq!(parse_unlock_timeout(Some("INF")), UnlockTimeout::Forever);
    }

    #[test]
    fn seconds_parsed_and_capped() {
        assert_eq!(parse_unlock_timeout(Some("30")), UnlockTimeout::Seconds(30));
        assert_eq!(
            parse_unlock_timeout(Some("999999")),
            UnlockTimeout::Seconds(MAX_UNLOCK_SECONDS)
        );
    }

    #[test]
    fn garbage_falls_back_to_default() {
        assert_eq!(parse_unlock_timeout(Some("abc")), UnlockTimeout::Default);
    }
}
