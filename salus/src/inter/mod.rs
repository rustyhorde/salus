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
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, NameType, ToFsName, ToNsName, tokio::Stream,
    traits::tokio::Stream as _,
};
use libsalus::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Builder, Clone, Debug)]
pub(crate) struct Inter {
    #[builder(into, default = "/var/run/salus.sock")]
    name: String,
}

impl Inter {
    pub(crate) async fn send(&self, message: Message) -> Result<()> {
        // Pick a name.
        let base_socket = "salus.sock";
        let ns_prefix = "/var/run/";
        let name = if GenericNamespaced::is_supported() {
            format!("{}{}", ns_prefix, base_socket).to_ns_name::<GenericNamespaced>()?
        } else {
            format!("/tmp/{}", base_socket).to_fs_name::<GenericFilePath>()?
        };

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
        let dec_res: Result<(Message, usize)> =
            decode_from_slice(&msg_buf, standard()).map_err(Into::into);

        match dec_res {
            Ok((msg, _size)) => match msg {
                Message::Success => {
                    println!("Operation successful");
                }
                Message::Error => {
                    println!("Operation failed");
                }
                _ => {
                    println!("Received unexpected message: {msg:?}");
                }
            },
            Err(e) => eprintln!("{e}"),
        }

        // Close the connection a bit earlier than you'd think we would. Nice practice!
        drop(recver);

        Ok(())
    }
}
