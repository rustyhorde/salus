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
use clap::Parser;
use interprocess::local_socket::{
    ListenerOptions,
    traits::tokio::{Listener, Stream as _},
};
use libsalus::{AgentAction, agent_socket_name, decode};
use tokio::{io::AsyncReadExt, spawn};
use tracing::{error, info, trace};

use crate::{
    config::{ConfigSalusAgent, load},
    error::Error,
    handler::AgentHandler,
    logging::initialize,
    runtime::cli::Cli,
    store::AgentState,
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

    // Load the configuration
    let config =
        load::<Cli, ConfigSalusAgent, Cli>(&cli, &cli).with_context(|| Error::ConfigLoad)?;

    // Initialize tracing
    initialize(&config, &config, &cli, None).with_context(|| Error::TracingInit)?;
    trace!("configuration loaded");
    trace!("tracing initialized");

    // Load every enrolled set from the OS keyring into memory.
    let store = Arc::new(Mutex::new(AgentState::load()?));
    trace!("enrolled sets loaded");

    // Setup the agent socket
    let name = agent_socket_name(config.socket_path().as_deref())?;
    let opts = ListenerOptions::new().name(name);
    let listener = match opts.create_tokio() {
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            error!(
                "Error: could not start the agent because the socket file is occupied. Please
                check if the socket is in use by another process and try again."
            );
            return Err(e.into());
        }
        x => x?,
    };

    info!("salus-agent is running");

    loop {
        let conn = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                error!("There was an error with an incoming connection: {e}");
                continue;
            }
        };

        let (mut receiver, sender) = conn.split();
        let store_c = store.clone();
        let cache_timeout = config.passphrase_cache_timeout();
        let _handle = spawn(async move {
            // Each request is a fresh connection carrying a single `AgentAction`;
            // the client half-closes its send side, so reading to EOF yields the
            // whole message.
            let mut msg_buf = Vec::new();
            if let Err(e) = receiver.read_to_end(&mut msg_buf).await {
                error!("Error reading agent request: {e}");
                return;
            }
            // A forged length prefix cannot trigger an unbounded allocation here:
            // `decode` enforces `MAX_MESSAGE_SIZE`. Malformed input is dropped.
            if let Ok(message) = decode::<AgentAction>(&msg_buf) {
                let mut handler = AgentHandler::builder()
                    .sender(sender)
                    .store(store_c)
                    .cache_timeout(cache_timeout)
                    .build();
                if let Err(e) = handler.handle(message).await {
                    error!("Error handling agent request: {e}");
                }
            }
        });
    }
}
