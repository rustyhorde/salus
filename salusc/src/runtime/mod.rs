// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::ffi::OsString;
use std::io::IsTerminal as _;

use anyhow::{Result, bail};
use clap::Parser;
use tokio::io::AsyncReadExt;

use crate::{
    config::load,
    inter::Inter,
    runtime::cli::{Cli, Commands},
};

mod cli;
mod generate;

pub(crate) async fn run<I, T>(args: Option<I>) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    // Parse the command line
    let cli = if let Some(args) = args {
        Cli::try_parse_from(args)?
    } else {
        Cli::try_parse()?
    };

    // Load the layered configuration (TOML file, SALUSC_ env vars, CLI flags).
    let config = load(&cli, cli.config_path())?;

    let inter = Inter::builder()
        .maybe_name(config.socket_path().map(String::from))
        .maybe_agent_name(config.agent_socket_path().map(String::from))
        .build();

    match cli.command() {
        Commands::Shares {
            num_shares,
            threshold,
        } => inter.shares(num_shares, threshold).await?,
        Commands::Unlock { set, duration } => inter.unlock(set, duration).await?,
        Commands::Lock => inter.lock().await?,
        Commands::Store {
            key,
            value,
            max_value_bytes,
            force,
        } => {
            const DEFAULT_MAX: usize = 65_536; // 64 KiB
            let max_bytes = max_value_bytes
                .or_else(|| config.store_max_value_bytes())
                .unwrap_or(DEFAULT_MAX);

            let value = if let Some(v) = value {
                v
            } else {
                if std::io::stdin().is_terminal() {
                    eprint!("Value: ");
                }
                let mut buf = String::new();
                let _ = tokio::io::stdin()
                    .take((max_bytes as u64).saturating_add(1))
                    .read_to_string(&mut buf)
                    .await?;
                if buf.len() > max_bytes {
                    bail!(
                        "stdin input exceeds {max_bytes} bytes; \
                         increase with --max-value-bytes or SALUSC_STORE_MAX_VALUE_BYTES"
                    );
                }
                if buf.ends_with('\n') {
                    let _ = buf.pop();
                    if buf.ends_with('\r') {
                        let _ = buf.pop();
                    }
                }
                buf
            };
            inter.store(key, value, force).await?;
        }

        Commands::Read { key } => inter.read(key).await?,
        Commands::Delete { key, force } => inter.delete(key, force).await?,
        Commands::Find { regex } => inter.find(regex).await?,
        Commands::Search { query, limit } => inter.search(query, limit).await?,
        Commands::Enroll {
            name,
            force,
            independent_auto,
        } => inter.enroll(name, force, independent_auto).await?,
        Commands::Forget { name, all, force } => inter.forget(name.as_deref(), all, force).await?,
        Commands::EnrollStatus => inter.enroll_status().await?,
        Commands::Gen {
            length,
            caps,
            numbers,
            special,
            passphrase,
            kind,
            key,
        } => {
            let secret = generate::generate(length, caps, numbers, special, passphrase, kind)?;
            if let Some(key) = key {
                inter.store(key, secret.clone(), false).await?;
            }
            generate::print_secret(&secret);
        }
    }

    Ok(())
}
