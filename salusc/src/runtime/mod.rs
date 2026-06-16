// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::ffi::OsString;

use anyhow::Result;
use clap::Parser;

use crate::{
    config::load,
    inter::{Inter, forget},
    runtime::cli::{Cli, Commands},
};

mod cli;

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
        Commands::Store { key, value } => inter.store(key, value).await?,
        Commands::Read { key_opt } => inter.read(key_opt).await?,
        Commands::Find { regex } => inter.find(regex).await?,
        Commands::Enroll {
            name,
            force,
            independent_auto,
        } => inter.enroll(name, force, independent_auto).await?,
        Commands::Forget { name, all } => forget(name.as_deref(), all)?,
        Commands::EnrollStatus => inter.enroll_status().await?,
    }

    Ok(())
}
