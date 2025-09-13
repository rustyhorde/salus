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
use crossterm::style::{Stylize, style};
use libsalus::{Action, Init, Response, Share};
use scanpw::scanpw;

use crate::{
    inter::Inter,
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

    let inter = Inter::builder().build();

    match cli.command() {
        Commands::Init {
            num_shares,
            threshold,
        } => {
            let init = Init::builder()
                .num_shares(num_shares)
                .threshold(threshold)
                .build();
            let message = Action::Init(init);
            if let Response::Error = inter.send(message).await? {
                eprintln!("Error occurred while initializing");
            }
        }
        Commands::Genkey => match inter.send(Action::Genkey).await? {
            Response::Shares(shares) => {
                println!(
                    "{}",
                    "These are your salus key shares.  Record them somewhere safe!  This will not be shown again.".green().bold(),
                );
                println!();
                for share in shares.shares() {
                    println!("{share}");
                }
            }
            Response::AlreadyInitialiazed => {
                println!("{}", "The share store is already initialized".red().bold());
            }
            _ => {
                println!("Received unexpected response");
            }
        },
        Commands::Unlock => {
            println!("{}", "Enter your 3 shares, one per prompt".green().bold());
            println!();
            for i in 0..3 {
                let share_in = scanpw!("{}", style(format!("Enter share {}/3: ", i + 1)).green());
                let share = Share::builder().share(share_in).build();
                let message = Action::Share(share);
                let _unused = inter.send(message).await?;
            }
            let _unused = inter.send(Action::Unlock).await?;
        }
    }

    Ok(())
}
