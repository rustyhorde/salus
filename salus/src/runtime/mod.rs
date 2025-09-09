// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{
    ffi::OsString,
    io::{self, BufRead as _},
};

use anyhow::Result;
use clap::Parser;
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, NameType, ToFsName, ToNsName, tokio::Stream,
    traits::tokio::Stream as _,
};
use libsalus::{SsssConfig, gen_key, unlock_key};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    try_join,
};

use crate::runtime::cli::{Cli, Commands};

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

    match cli.command() {
        Commands::Init {
            num_shares: _,
            threshold: _,
        } => {}
        Commands::Genkey => {
            let shares = gen_key(&SsssConfig::default())?;
            println!(
                "These are your salus key shares.  Record them somewhere safe!  This will not be shown again."
            );
            println!();
            for share in shares {
                println!("{share}");
            }
        }
        Commands::Unlock => {
            let stdin = io::stdin();
            let mut iterator = stdin.lock().lines();
            let mut shares = Vec::new();
            for _ in 0..3 {
                if let Some(Ok(line)) = iterator.next() {
                    shares.push(line);
                } else {
                    break;
                }
            }
            let key = unlock_key(&shares)?;
            let hex_string = key
                .iter()
                .fold(String::new(), |b, byte| format!("{b}{byte:02X}"));
            println!("Key unlocked successfully. {hex_string}");
        }
    }

    // Pick a name.
    let name = if GenericNamespaced::is_supported() {
        "example.sock".to_ns_name::<GenericNamespaced>()?
    } else {
        "/tmp/example.sock".to_fs_name::<GenericFilePath>()?
    };

    // Await this here since we can't do a whole lot without a connection.
    let conn = Stream::connect(name).await?;

    // This consumes our connection and splits it into two halves, so that we can concurrently use
    // both.
    let (recver, mut sender) = conn.split();
    let mut recver = BufReader::new(recver);

    // Allocate a sizeable buffer for receiving. This size should be enough and should be easy to
    // find for the allocator.
    let mut buffer = String::with_capacity(128);

    // Describe the send operation as writing our whole string.
    let send = sender.write_all(b"Hello from client!\n");
    // Describe the receive operation as receiving until a newline into our buffer.
    let recv = recver.read_line(&mut buffer);

    // Concurrently perform both operations.
    try_join!(send, recv)?;

    // Close the connection a bit earlier than you'd think we would. Nice practice!
    drop((recver, sender));

    // Display the results when we're done!
    println!("Server answered: {}", buffer.trim());

    Ok(())
}
