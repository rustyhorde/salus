// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::io::{IsTerminal as _, Write, stderr, stdin, stdout};

use anyhow::{Context, Result, bail};
use bon::Builder;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers, read},
    queue,
    style::{Color, Print, Stylize, style},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, size,
    },
};
use interprocess::local_socket::{tokio::Stream, traits::tokio::Stream as _};
use libsalus::{
    Action, AgentAction, AgentResponse, MAX_UNLOCK_SECONDS, Response, SearchQuery, SetInfo, Share,
    Store, UnlockTimeout, agent_socket_name, decode, encode, socket_name,
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
        // An empty buffer means the daemon closed the connection without writing a
        // response (e.g. it could not decode our request because it predates an
        // action this client now sends). Surface that clearly instead of letting
        // `decode` fail with an opaque `UnexpectedEnd`.
        if msg_buf.is_empty() {
            bail!(
                "salusd closed the connection without responding; it may be out of date — restart or reinstall the daemon"
            );
        }
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

        // Refresh the running agent so the newly enrolled set is offered at the
        // next unlock without waiting for an agent restart.
        self.reload_agent().await;
        println!("{}", format!("Enrolled set '{name}'.").green().bold());
        Ok(())
    }

    /// Remove a named enrolled set, or every set when `all` is set.
    ///
    /// Confirms by default; pass `force` to skip the prompt. After a change, the
    /// running agent is asked to re-read its sets so a forgotten set stops being
    /// offered at unlock without an agent restart.
    ///
    /// # Errors
    ///
    /// Returns an error if neither a name nor `all` is given, or a keyring
    /// operation fails.
    pub(crate) async fn forget(&self, name: Option<&str>, all: bool, force: bool) -> Result<()> {
        if !all && name.is_none() {
            bail!("specify --name <set> to remove one set, or --all to remove every set");
        }

        // Confirm by default. A destructive forget should never proceed without
        // an explicit yes: when stdin is not a terminal we cannot prompt, so a
        // non-interactive forget must pass `--force` rather than be silently
        // confirmed by piped input.
        if !force {
            if !stdin().is_terminal() {
                eprintln!(
                    "{}",
                    "Refusing to forget without confirmation; \
                     re-run with --force for non-interactive use"
                        .red()
                        .bold()
                );
                return Ok(());
            }
            let prompt = if all {
                "Forget ALL enrolled sets? [y/N]: ".to_string()
            } else if let Some(name) = name {
                format!("Forget enrolled set '{name}'? [y/N]: ")
            } else {
                // Unreachable: the guard above rejects the no-target case.
                "Forget enrolled set? [y/N]: ".to_string()
            };
            let answer = prompt_line(&prompt)?;
            let answer = answer.trim().to_ascii_lowercase();
            if answer != "y" && answer != "yes" {
                println!("{}", "Aborted; nothing was forgotten.".yellow());
                return Ok(());
            }
        }

        if all {
            keystore::forget_all()?;
            self.reload_agent().await;
            println!("{}", "Removed all enrolled sets.".green().bold());
        } else if let Some(name) = name {
            if keystore::forget(name)? {
                self.reload_agent().await;
                println!(
                    "{}",
                    format!("Removed enrolled set '{name}'.").green().bold()
                );
            } else {
                eprintln!("{}", format!("No enrolled set named '{name}'.").yellow());
            }
        }
        Ok(())
    }

    /// Ask the running agent (if any) to re-read its enrolled sets from the
    /// keyring.
    ///
    /// Best-effort: an unreachable agent is fine — it loads a fresh view at its
    /// next start — so any error is swallowed rather than surfaced.
    async fn reload_agent(&self) {
        let _ignored = self.agent_send(AgentAction::Reload).await;
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

    /// Send a single predictive-search request and return the ranked matches.
    ///
    /// A daemon-side error (for example, `StoreNotUnlocked`) is surfaced as an
    /// `Err` so callers can decide how to present it.
    async fn send_search(&self, query: &str, limit: Option<usize>) -> Result<Vec<String>> {
        let message = Action::Search(
            SearchQuery::builder()
                .query(query)
                .maybe_limit(limit)
                .build(),
        );
        match self.send(message).await? {
            Response::Matches(matches) => Ok(matches),
            Response::Error(error) => bail!("{error}"),
            _ => bail!("Unexpected response from salusd"),
        }
    }

    /// Predictively search stored key names.
    ///
    /// With a `query`, prints the ranked matches once. Without one, opens an
    /// interactive filter prompt.
    pub(crate) async fn search(&self, query: Option<String>, limit: Option<usize>) -> Result<()> {
        match query {
            Some(query) => self.search_once(&query, limit).await,
            None => self.search_interactive(limit).await,
        }
    }

    /// One-shot search: print the ranked matches, reusing the `find` styling.
    async fn search_once(&self, query: &str, limit: Option<usize>) -> Result<()> {
        match self.send_search(query, limit).await {
            Ok(matches) => {
                if matches.is_empty() {
                    let no_match_style = style(format!("No keys matched '{query}'")).red().bold();
                    println!("{no_match_style}");
                } else {
                    println!("{}", "Matching keys:".green().bold());
                    for key in matches {
                        let key_style = style(key).with(Color::Green).bold();
                        println!("{key_style}");
                    }
                }
            }
            Err(e) => {
                eprintln!("Error occurred while searching keys: {e}");
            }
        }
        Ok(())
    }

    /// Interactive live-filtering prompt: type to narrow, arrows to move, Enter
    /// to print the selected key, Esc/Ctrl-C to cancel.
    async fn search_interactive(&self, limit: Option<usize>) -> Result<()> {
        if !stdin().is_terminal() || !stderr().is_terminal() {
            bail!("Interactive search requires a terminal; pass a QUERY argument instead");
        }

        // Fetch the initial (unfiltered) list first. This also surfaces a locked
        // store before we ever switch the terminal into raw mode.
        let mut matches = match self.send_search("", limit).await {
            Ok(matches) => matches,
            Err(e) => {
                eprintln!("Error occurred while searching keys: {e}");
                return Ok(());
            }
        };

        let mut query = String::new();
        let mut selection = 0usize;

        let guard = TermGuard::enter()?;
        let selected = loop {
            render_prompt(&matches, selection, &query)?;
            match read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                            break None;
                        }
                        (KeyCode::Enter, _) => break matches.get(selection).cloned(),
                        (KeyCode::Up, _) => selection = selection.saturating_sub(1),
                        (KeyCode::Down, _) => {
                            let max = matches.len().saturating_sub(1);
                            selection = selection.saturating_add(1).min(max);
                        }
                        (KeyCode::Backspace, _) => {
                            let _ = query.pop();
                            matches = self.send_search(&query, limit).await.unwrap_or_default();
                            selection = 0;
                        }
                        (KeyCode::Char(c), mods)
                            if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            query.push(c);
                            matches = self.send_search(&query, limit).await.unwrap_or_default();
                            selection = 0;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        };
        // Restore the terminal before any stdout output.
        drop(guard);

        if let Some(key) = selected {
            println!("{key}");
        }
        Ok(())
    }
}

/// Restores the terminal (raw mode + alternate screen + cursor) on scope exit.
///
/// The no-panic rule means cleanup cannot rely on unwinding, so a guard
/// guarantees the terminal is returned to a sane state even on an early return
/// or error.
struct TermGuard;

impl TermGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        queue!(stderr(), EnterAlternateScreen, Hide)?;
        stderr().flush()?;
        Ok(TermGuard)
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        // Best-effort restore; nothing actionable if these fail during teardown.
        drop(queue!(stderr(), Show, LeaveAlternateScreen));
        drop(stderr().flush());
        drop(disable_raw_mode());
    }
}

/// Draw the interactive search prompt (query line + ranked candidate list) to
/// the alternate screen on stderr, keeping the selected row visible.
fn render_prompt(matches: &[String], selection: usize, query: &str) -> Result<()> {
    let mut out = stderr();
    let (_, rows) = size().unwrap_or((80, 24));
    // Reserve one row for the prompt line and one for breathing room.
    let visible = usize::from(rows).saturating_sub(2).max(1);
    let start = if selection >= visible {
        selection.saturating_sub(visible).saturating_add(1)
    } else {
        0
    };

    queue!(out, MoveTo(0, 0), Clear(ClearType::All))?;
    let header = style(format!("search> {query}")).with(Color::Cyan).bold();
    queue!(out, Print(format!("{header}\r\n")))?;

    if matches.is_empty() {
        let empty = style("  (no matching keys)").with(Color::DarkGrey);
        queue!(out, Print(format!("{empty}\r\n")))?;
    } else {
        for (idx, key) in matches.iter().enumerate().skip(start).take(visible) {
            let line = if idx == selection {
                style(format!("> {key}"))
                    .with(Color::Black)
                    .on(Color::Green)
            } else {
                style(format!("  {key}")).with(Color::Green)
            };
            queue!(out, Print(format!("{line}\r\n")))?;
        }
    }
    out.flush()?;
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
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use anyhow::Result;
    use interprocess::local_socket::{
        GenericFilePath, ListenerOptions, ToFsName,
        traits::tokio::{Listener, Stream as _},
    };
    use libsalus::{
        Action, AgentAction, AgentResponse, MAX_UNLOCK_SECONDS, Response, SetInfo, Shares,
        UnlockTimeout, decode, encode,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        task::JoinHandle,
    };

    use super::{Inter, parse_set_choice, parse_unlock_timeout, render_prompt};

    /// Allocate a unique filesystem socket path so parallel tests never collide.
    fn unique_socket_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("salus-test-{}-{tag}-{n}.sock", std::process::id()))
    }

    /// Build an `Inter` pointed at the given daemon socket path. The agent socket
    /// is pointed at a path with no listener so agent probes fail fast.
    fn inter_for(path: &Path) -> Inter {
        Inter::builder()
            .name(path.to_string_lossy().into_owned())
            .agent_name(unique_socket_path("noagent").to_string_lossy().into_owned())
            .build()
    }

    /// Stand up a mock daemon that accepts one connection per queued response,
    /// reads the incoming `Action`, and writes back the canned `Response`. The
    /// returned handle yields the `Action`s the client actually sent.
    fn spawn_daemon_mock(
        path: &Path,
        responses: Vec<Response>,
    ) -> Result<JoinHandle<Result<Vec<Action>>>> {
        let name = path.to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        Ok(tokio::spawn(async move {
            let mut received = Vec::new();
            for response in responses {
                let conn = listener.accept().await?;
                let (mut recver, mut sender) = conn.split();
                let mut buf = Vec::new();
                let _n = recver.read_to_end(&mut buf).await?;
                received.push(decode::<Action>(&buf)?);
                let bytes = encode(response)?;
                sender.write_all(&bytes).await?;
                sender.flush().await?;
                drop(sender);
            }
            Ok(received)
        }))
    }

    /// Like [`spawn_daemon_mock`] but speaks the `salus-agent` protocol.
    fn spawn_agent_mock(
        path: &Path,
        responses: Vec<AgentResponse>,
    ) -> Result<JoinHandle<Result<Vec<AgentAction>>>> {
        let name = path.to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        Ok(tokio::spawn(async move {
            let mut received = Vec::new();
            for response in responses {
                let conn = listener.accept().await?;
                let (mut recver, mut sender) = conn.split();
                let mut buf = Vec::new();
                let _n = recver.read_to_end(&mut buf).await?;
                received.push(decode::<AgentAction>(&buf)?);
                let bytes = encode(response)?;
                sender.write_all(&bytes).await?;
                sender.flush().await?;
                drop(sender);
            }
            Ok(received)
        }))
    }

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

    #[tokio::test]
    async fn send_round_trips_action_and_response() -> Result<()> {
        let path = unique_socket_path("send");
        let handle = spawn_daemon_mock(&path, vec![Response::Success])?;
        let inter = inter_for(&path);

        assert!(matches!(inter.send(Action::Lock).await?, Response::Success));

        let received = handle.await??;
        assert!(matches!(received.as_slice(), [Action::Lock]));
        Ok(())
    }

    #[tokio::test]
    async fn send_reports_empty_response_clearly() -> Result<()> {
        // Simulate a daemon that closes the connection without writing a
        // response (e.g. an older daemon that could not decode the action). The
        // client must surface a clear error, not an opaque bincode `UnexpectedEnd`.
        let path = unique_socket_path("send-empty");
        let name = path.as_path().to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        let handle = tokio::spawn(async move {
            let conn = listener.accept().await?;
            let (mut recver, sender) = conn.split();
            let mut buf = Vec::new();
            let _n = recver.read_to_end(&mut buf).await?;
            drop(sender); // close without responding
            Ok::<(), anyhow::Error>(())
        });

        let result = inter_for(&path).send(Action::Lock).await;
        assert!(result.is_err(), "empty response should be an error");
        if let Err(e) = result {
            assert!(e.to_string().contains("closed the connection"));
        }

        handle.await??;
        Ok(())
    }

    #[tokio::test]
    async fn shares_handles_every_response_arm() -> Result<()> {
        for response in [
            Response::Shares(Shares::builder().shares(vec!["s1".to_string()]).build()),
            Response::AlreadyInitialiazed,
            Response::Error("boom".to_string()),
            Response::Success, // unexpected arm
        ] {
            let path = unique_socket_path("shares");
            let _handle = spawn_daemon_mock(&path, vec![response])?;
            inter_for(&path).shares(5, 3).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn lock_handles_success_and_error() -> Result<()> {
        for response in [Response::Success, Response::Error("nope".to_string())] {
            let path = unique_socket_path("lock");
            let _handle = spawn_daemon_mock(&path, vec![response])?;
            inter_for(&path).lock().await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn find_handles_matches_and_error() -> Result<()> {
        for response in [
            Response::Matches(vec!["aws-prod".to_string()]),
            Response::Matches(vec![]),
            Response::Error("bad regex".to_string()),
        ] {
            let path = unique_socket_path("find");
            let _handle = spawn_daemon_mock(&path, vec![response])?;
            inter_for(&path).find("aws.*".to_string()).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn read_handles_every_response_arm() -> Result<()> {
        for response in [
            Response::Value(Some(b"plain".to_vec())),
            Response::Value(Some(vec![0xff, 0xfe, 0x00])), // non-UTF-8
            Response::Value(None),
            Response::KeyNotFound,
            Response::Error("read failed".to_string()),
        ] {
            let path = unique_socket_path("read");
            let _handle = spawn_daemon_mock(&path, vec![response])?;
            inter_for(&path).read("k".to_string()).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn store_success_and_error() -> Result<()> {
        for response in [Response::Success, Response::Error("disk full".to_string())] {
            let path = unique_socket_path("store");
            let _handle = spawn_daemon_mock(&path, vec![response])?;
            inter_for(&path)
                .store("k".to_string(), "v".to_string(), false)
                .await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn store_key_exists_refuses_without_terminal() -> Result<()> {
        // Under `cargo test` stdin is not a terminal, so a `KeyExists` response
        // takes the non-interactive "refuse to overwrite" branch and makes no
        // second request.
        let path = unique_socket_path("store-exists");
        let handle = spawn_daemon_mock(&path, vec![Response::KeyExists])?;
        inter_for(&path)
            .store("k".to_string(), "v".to_string(), false)
            .await?;
        let received = handle.await??;
        assert_eq!(received.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn delete_with_force_handles_arms() -> Result<()> {
        for response in [
            Response::Success,
            Response::KeyNotFound,
            Response::Error("locked".to_string()),
        ] {
            let path = unique_socket_path("delete");
            let _handle = spawn_daemon_mock(&path, vec![response])?;
            inter_for(&path).delete("k".to_string(), true).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn delete_without_force_refuses_without_terminal() -> Result<()> {
        // Non-terminal stdin + no `--force` means the delete is refused before any
        // request is sent, so no mock daemon is needed.
        let path = unique_socket_path("delete-refuse");
        inter_for(&path).delete("k".to_string(), false).await?;
        Ok(())
    }

    #[tokio::test]
    async fn send_search_surfaces_matches_and_errors() -> Result<()> {
        let path = unique_socket_path("search-ok");
        let _handle = spawn_daemon_mock(&path, vec![Response::Matches(vec!["aws".to_string()])])?;
        let matches = inter_for(&path).send_search("aws", None).await?;
        assert_eq!(matches, vec!["aws".to_string()]);

        let path = unique_socket_path("search-err");
        let _handle = spawn_daemon_mock(&path, vec![Response::Error("locked".to_string())])?;
        assert!(inter_for(&path).send_search("aws", None).await.is_err());

        let path = unique_socket_path("search-unexpected");
        let _handle = spawn_daemon_mock(&path, vec![Response::Success])?;
        assert!(inter_for(&path).send_search("aws", None).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn search_once_prints_matches_and_empty() -> Result<()> {
        let path = unique_socket_path("search-once");
        let _handle =
            spawn_daemon_mock(&path, vec![Response::Matches(vec!["github".to_string()])])?;
        inter_for(&path)
            .search(Some("git".to_string()), Some(10))
            .await?;

        let path = unique_socket_path("search-once-empty");
        let _handle = spawn_daemon_mock(&path, vec![Response::Matches(vec![])])?;
        inter_for(&path)
            .search(Some("zzz".to_string()), None)
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn agent_send_round_trips() -> Result<()> {
        let path = unique_socket_path("agent");
        let handle = spawn_agent_mock(&path, vec![AgentResponse::Status { sets: vec![] }])?;
        let inter = Inter::builder()
            .name(
                unique_socket_path("nodaemon")
                    .to_string_lossy()
                    .into_owned(),
            )
            .agent_name(path.to_string_lossy().into_owned())
            .build();

        assert!(matches!(
            inter.agent_send(AgentAction::Status).await?,
            AgentResponse::Status { .. }
        ));
        let received = handle.await??;
        assert!(matches!(received.as_slice(), [AgentAction::Status]));
        Ok(())
    }

    #[tokio::test]
    async fn collect_shares_falls_back_when_agent_unreachable() -> Result<()> {
        // `inter_for` points the agent at a socket with no listener.
        let path = unique_socket_path("collect-unreachable");
        let result = inter_for(&path).collect_shares_via_agent(None).await?;
        assert!(result.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn collect_shares_falls_back_when_no_sets() -> Result<()> {
        let agent = unique_socket_path("collect-empty-agent");
        let _handle = spawn_agent_mock(&agent, vec![AgentResponse::Status { sets: vec![] }])?;
        let inter = Inter::builder()
            .name(
                unique_socket_path("nodaemon")
                    .to_string_lossy()
                    .into_owned(),
            )
            .agent_name(agent.to_string_lossy().into_owned())
            .build();
        assert!(inter.collect_shares_via_agent(None).await?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn collect_shares_falls_back_on_unknown_set() -> Result<()> {
        let agent = unique_socket_path("collect-unknown-agent");
        let _handle = spawn_agent_mock(
            &agent,
            vec![
                AgentResponse::Status {
                    sets: vec![SetInfo {
                        name: "prod".to_string(),
                        auto_count: 2,
                    }],
                },
                AgentResponse::UnknownSet,
            ],
        )?;
        let inter = Inter::builder()
            .name(
                unique_socket_path("nodaemon")
                    .to_string_lossy()
                    .into_owned(),
            )
            .agent_name(agent.to_string_lossy().into_owned())
            .build();
        // An explicit (missing) set name avoids the interactive set chooser.
        assert!(
            inter
                .collect_shares_via_agent(Some("missing".to_string()))
                .await?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn render_prompt_handles_populated_and_empty_lists() -> Result<()> {
        let matches = vec![
            "aws-prod".to_string(),
            "aws-staging".to_string(),
            "github".to_string(),
        ];
        render_prompt(&matches, 1, "aws")?;
        render_prompt(&[], 0, "zzz")?;
        Ok(())
    }
}
