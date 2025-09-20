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
use crossterm::style::{Color, Stylize, style};
use libsalus::{Action, Response, Share, Store};
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
        Commands::Shares {
            num_shares,
            threshold,
        } => match inter.send(Action::GenShares(num_shares, threshold)).await? {
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
        },
        Commands::Unlock => {
            let mut threshold = 3;
            if let Response::Threshold(th) = inter.send(Action::GetThreshold).await? {
                threshold = th;
            }

            let th_prompt = format!("Enter your {threshold} shares, one per prompt");
            println!("{}", th_prompt.green().bold());
            println!();
            for i in 0..threshold {
                let share_in = scanpw!(
                    "{}",
                    style(format!("Enter share {}/{threshold}: ", i + 1)).green()
                );
                let share = Share::builder().share(share_in).build();
                let message = Action::Share(share);
                let _unused = inter.send(message).await?;
            }
            let _unused = inter.send(Action::Unlock).await?;
        }
        Commands::Store { key, value } => {
            let message = Action::Store(Store::builder().key(key).value(value).build());
            if let Response::Error(error) = inter.send(message).await? {
                eprintln!("Error occurred while storing value: {error}");
            }
        }
        Commands::Read { key_opt } => {
            // TODO: if key is not provided, prompt for it
            // println!("Enter the key you wish to read: ");
            // let mut key = String::new();
            // let stdin = io::stdin();
            // let _bytes_read = stdin
            //     .lock()
            //     .read_line(&mut key)
            //     .with_context(|| "Failed to read line")?;
            // let key = key.trim().to_string();
            if let Some(key) = key_opt {
                let message = Action::Read(key.clone());
                match inter.send(message).await? {
                    Response::Value(value) => {
                        if let Some(val) = value {
                            let value_style = style(val).with(Color::Green).bold();
                            println!("{}", "Value: ".green());
                            println!("{value_style}");
                        } else {
                            let not_found_style =
                                style(format!("No value found for '{key}'")).red().bold();
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
            }
        }
    }

    Ok(())
}
