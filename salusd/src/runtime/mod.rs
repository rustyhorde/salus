// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{
    ffi::OsString,
    io::ErrorKind,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use bincode::{config::standard, decode_from_slice};
use clap::Parser;
use interprocess::local_socket::{
    ListenerOptions,
    traits::tokio::{Listener, RecvHalf, Stream as _},
};
use libsalus::{Action, socket_name};
use tokio::{
    io::AsyncReadExt,
    spawn,
    sync::mpsc::{UnboundedSender, unbounded_channel},
};
use tracing::{error, info, trace};

use crate::{
    config::{ConfigSalusd, load},
    db::initialize_redb,
    error::Error,
    handler::ActionHandler,
    logging::initialize,
    runtime::cli::Cli,
    store::ShareStore,
};

mod cli;

#[allow(clippy::too_many_lines)]
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

    // Load the configuration
    let config = load::<Cli, ConfigSalusd, Cli>(&cli, &cli).with_context(|| Error::ConfigLoad)?;

    // Initialize tracing
    initialize(&config, &config, &cli, None).with_context(|| Error::TracingInit)?;

    trace!("configuration loaded");
    trace!("tracing initialized");

    // Initialize the database
    let redb = initialize_redb(&cli).with_context(|| Error::DatabaseInit)?;
    trace!("database initialized");

    // Setup the socket
    let (_base_name, name) = socket_name()?;
    trace!("socket setup");

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
            error!(
                "Error: could not start server because the socket file is occupied. Please check
                if the socket is in use by another process and try again."
            );
            return Err(e.into());
        }
        x => x?,
    };

    // The syncronization between the server and client, if any is used, goes here.
    info!("salusd daemon is running");

    // Set up our share store and the message handler for it.
    let share_store = Arc::new(Mutex::new(ShareStore::builder().redb(redb.clone()).build()));

    // Set up our loop boilerplate that processes our incoming connections.
    loop {
        let conn = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                error!("There was an error with an incoming connection: {e}");
                continue;
            }
        };

        let (mut receiver, sender) = conn.split();
        let (tx, mut rx) = unbounded_channel::<Action>();
        let share_store_c = share_store.clone();
        let _client_recv_handle = spawn(async move {
            let mut action_handler = ActionHandler::builder()
                .sender(sender)
                .store(share_store_c)
                .build();
            while let Some(message) = rx.recv().await {
                if let Err(e) = action_handler.action_handler(message).await {
                    error!("Error handling client message: {e}");
                }
            }
        });

        let _handle = spawn(async move {
            if let Err(e) = handle_conn(&mut receiver, tx).await {
                error!("Error while handling connection: {e}");
            }
        });
    }
}

async fn handle_conn<T: RecvHalf + Unpin>(
    receiver: &mut T,
    txc: UnboundedSender<Action>,
) -> Result<()> {
    // Describe the receive operation as receiving a line into our big buffer.
    let mut msg_buf = Vec::new();
    let _msg_size = receiver.read_to_end(&mut msg_buf).await?;

    let decoded_res: Result<(Action, usize)> =
        decode_from_slice(&msg_buf, standard()).map_err(Into::into);
    if let Ok((message, _)) = decoded_res {
        txc.send(message)?;
    }

    Ok(())
}
