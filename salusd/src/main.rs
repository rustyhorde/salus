// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::io::ErrorKind;

use anyhow::Result;
use bincode::{config::standard, decode_from_slice, encode_to_vec};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, NameType, ToFsName, ToNsName,
    traits::tokio::{Listener, RecvHalf, Stream as _},
};
use libsalus::Message;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Pick a name.
    let base_socket = "salus.sock";
    let ns_prefix = "/var/run/";
    let name = if GenericNamespaced::is_supported() {
        format!("{ns_prefix}{base_socket}").to_ns_name::<GenericNamespaced>()?
    } else {
        format!("/tmp/{base_socket}").to_fs_name::<GenericFilePath>()?
    };

    // Configure our listener...
    let opts = ListenerOptions::new().name(name);

    // ...and create it.
    let listener = match opts.create_tokio() {
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            // When a program that uses a file-type socket name terminates its socket server
            // without deleting the file, a "corpse socket" remains, which can neither be
            // connected to nor reused by a new listener. Normally, Interprocess takes care of
            // this on affected platforms by deleting the socket file when the listener is
            // dropped. (This is vulnerable to all sorts of races and thus can be disabled.)
            //
            // There are multiple ways this error can be handled, if it occurs, but when the
            // listener only comes from Interprocess, it can be assumed that its previous instance
            // either has crashed or simply hasn't exited yet. In this example, we leave cleanup
            // up to the user, but in a real application, you usually don't want to do that.
            eprintln!(
                "Error: could not start server because the socket file is occupied. Please check
                if {base_socket} is in use by another process and try again."
            );
            return Err(e.into());
        }
        x => x?,
    };

    // The syncronization between the server and client, if any is used, goes here.
    eprintln!("Server running at {base_socket}");

    // Set up our loop boilerplate that processes our incoming connections.
    loop {
        // Sort out situations when establishing an incoming connection caused an error.
        let conn = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("There was an error with an incoming connection: {e}");
                continue;
            }
        };

        let (mut receiver, mut sender) = conn.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

        let _client_recv_handle = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                eprintln!("Got a message from a client: {message:?}");
                match message {
                    Message::Share(share) => {
                        eprintln!("Share received: {}", share.share());
                    }
                    Message::Unlock => {
                        eprintln!("Unlock requested, but not implemented.");
                    }
                    _ => {}
                }
                let mut blah = async || -> Result<()> {
                    let message = encode_to_vec(Message::Success, standard())?;
                    sender.write_all(&message).await?;
                    sender.flush().await?;
                    Ok(())
                };
                if let Err(e) = blah().await {
                    eprintln!("There was an error when sending: {e}");
                }
            }
        });

        // Spawn new parallel asynchronous tasks onto the Tokio runtime and hand the connection
        // over to them so that multiple clients could be processed simultaneously in a
        // lightweight fashion.
        tokio::spawn(async move {
            // The outer match processes errors that happen when we're connecting to something.
            // The inner if-let processes errors that happen during the connection.
            eprintln!("Got a connection!");
            if let Err(e) = handle_conn(&mut receiver, tx).await {
                eprintln!("Error while handling connection: {e}");
            }
            eprintln!("Connection closed.");
        });
    }
}

// Describe the things we do when we've got a connection ready.
async fn handle_conn<T: RecvHalf + Unpin>(
    receiver: &mut T,
    txc: mpsc::UnboundedSender<Message>,
) -> Result<()> {
    // Describe the receive operation as receiving a line into our big buffer.
    let mut msg_buf = Vec::new();
    let _msg_size = receiver.read_to_end(&mut msg_buf).await?;

    let decoded_res: Result<(Message, usize)> =
        decode_from_slice(&msg_buf, standard()).map_err(Into::into);
    if let Ok((message, _)) = decoded_res {
        println!("Client sent: {:?}", message);
        txc.send(message)?;
    }

    Ok(())
}
