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
use interprocess::local_socket::{tokio::Stream, traits::tokio::Stream as _};
use libsalus::{Action, Response, socket_name};
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

        // Allocate a sizeable buffer for receiving. This size should be enough and should be easy to
        // find for the allocator.
        // let mut buffer = String::with_capacity(128);

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
}
