// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::Result;
use bincode::{config::standard, decode_from_slice, encode_to_vec};
use bon::Builder;
use crossterm::style::{Color, Stylize, style};
use interprocess::local_socket::{tokio::Stream, traits::tokio::Stream as _};
use libsalus::{Action, Response, Share, Store, socket_name};
use scanpw::scanpw;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Builder, Clone, Debug)]
pub(crate) struct Inter {
    #[builder(into, default = "/var/run/salus.sock")]
    #[allow(dead_code)]
    name: String,
}

impl Inter {
    pub(crate) async fn send(&self, message: Action) -> Result<Response> {
        // Pick a name.
        let (_base_name, name) = socket_name()?;

        // Await this here since we can't do a whole lot without a connection.
        let conn = Stream::connect(name).await?;

        // This consumes our connection and splits it into two halves, so that we can concurrently use
        // both.
        let (recver, mut sender) = conn.split();
        let mut recver = BufReader::new(recver);

        // Describe the send operation as writing our whole string.
        let _handle = tokio::spawn(async move {
            let blah = async || -> Result<()> {
                let message = encode_to_vec(message, standard())?;
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
        let dec_res: Result<(Response, usize)> =
            decode_from_slice(&msg_buf, standard()).map_err(Into::into);

        match dec_res {
            Ok((msg, _size)) => Ok(msg),
            Err(e) => Err(e),
        }
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

    pub(crate) async fn unlock(&self) -> Result<()> {
        let mut threshold = 3;
        if let Response::Threshold(th) = self.send(Action::GetThreshold).await? {
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
            let _unused = self.send(message).await?;
        }
        let _unused = self.send(Action::Unlock).await?;
        Ok(())
    }

    pub(crate) async fn store(&self, key: String, value: String) -> Result<()> {
        let message = Action::Store(Store::builder().key(key).value(value).build());
        if let Response::Error(error) = self.send(message).await? {
            eprintln!("Error occurred while storing value: {error}");
        }
        Ok(())
    }

    pub(crate) async fn read(&self, key_opt: Option<String>) -> Result<()> {
        // TODO: if key is not provided, prompt for it
        if let Some(key) = key_opt {
            let message = Action::Read(key.clone());
            match self.send(message).await? {
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
        Ok(())
    }

    pub(crate) async fn find(&self, regex: String) -> Result<()> {
        let message = Action::FindKey(regex.clone());
        match self.send(message).await? {
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
