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
use aws_lc_rs::aead::{AES_256_GCM, Aad, Nonce, RandomizedNonceKey};
use bincode::{config::standard, decode_from_slice};
use clap::Parser;
use interprocess::local_socket::{
    ListenerOptions,
    traits::tokio::{Listener, RecvHalf, Stream as _},
};
use libsalus::{Action, socket_name, unlock_key};
use tokio::{
    io::AsyncReadExt,
    spawn,
    sync::mpsc::{UnboundedSender, unbounded_channel},
    time::{Duration, sleep},
};
use tracing::{error, info, trace, warn};

use crate::{
    config::{ConfigSalusd, load},
    db::{SALUS_VAL_TABLE_DEF, SalusVal, initialize_redb, read_value, unlock_redb, write_value},
    error::Error,
    handler::{ShareStore, ShareStoreMessage, handler},
    logging::initialize,
    runtime::cli::Cli,
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
    trace!("Socket setup");

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
    let share_store = Arc::new(Mutex::new(ShareStore::default()));
    let (stx, mut srx) = unbounded_channel::<ShareStoreMessage>();
    let stx_c = stx.clone();
    let redb_c = redb.clone();
    let _store_message_handler = spawn(async move {
        while let Some(ssm) = srx.recv().await {
            match ssm {
                ShareStoreMessage::AddShare(share) => {
                    unlock_store(&share_store, |store: &mut ShareStore| {
                        store.add_share(share.clone());
                    });
                }
                ShareStoreMessage::Unlock => {
                    unlock_store(&share_store, |store: &mut ShareStore| {
                        match unlock_key(&store.shares()) {
                            Ok(key) => {
                                match read_value::<String, SalusVal>(
                                    &redb_c.lock().unwrap(),
                                    SALUS_VAL_TABLE_DEF,
                                    "CHECK_KEY".to_string(),
                                ) {
                                    Err(e) => {
                                        error!("Error reading CHECK_KEY from database: {e}");
                                        return;
                                    }
                                    Ok(None) => {
                                        error!("CHECK_KEY not found in database");
                                        return;
                                    }
                                    Ok(Some(svag)) => {
                                        let sv = svag.value();
                                        let nonce = Nonce::from(sv.nonce());
                                        let rnkey = RandomizedNonceKey::new(&AES_256_GCM, &key)
                                            .with_context(|| Error::NonceKeyGen)
                                            .unwrap();
                                        let mut ciphertext = sv.ciphertext().clone();
                                        let plaintext_b = rnkey
                                            .open_in_place(nonce, Aad::empty(), &mut ciphertext)
                                            .with_context(|| Error::NonceKeyGen)
                                            .unwrap();
                                        let plaintext =
                                            String::from_utf8_lossy(plaintext_b).to_string();
                                        trace!(
                                            "Unlocked key with shares, got plaintext: {plaintext}"
                                        );
                                        if plaintext == "CHECK_KEY" {
                                            info!("Key successfully unlocked and verified.");
                                            let interval = sleep(Duration::from_secs(20));
                                            let stx_c = stx_c.clone();
                                            let _blah = spawn(async move {
                                                interval.await;
                                                stx_c.send(ShareStoreMessage::ClearKey).unwrap();
                                            });
                                            store.add_key(key);
                                        } else {
                                            error!("Failed to unlock key with provided shares");
                                        }
                                    }
                                }
                            }
                            Err(e) => error!("Failed to unlock key with provided shares: {e}"),
                        }
                        store.clear_shares();
                    });
                }
                ShareStoreMessage::ClearKey => {
                    unlock_store(&share_store, |store: &mut ShareStore| {
                        warn!("Key cleared due to timeout.");
                        store.clear_key();
                    });
                }
                ShareStoreMessage::Init => {
                    warn!("Initialize requested, but not implemented.");
                }
                ShareStoreMessage::Store(store_val) => {
                    unlock_store(&share_store, |store: &mut ShareStore| {
                        let key_val = store_val.key();
                        let mut value = store_val.value().as_bytes().to_vec();

                        if let Some(key) = store.key() {
                            let nonce_key = RandomizedNonceKey::new(&AES_256_GCM, &key)
                                .with_context(|| Error::NonceKeyGen)
                                .unwrap();
                            let nonce = nonce_key
                                .seal_in_place_append_tag(Aad::empty(), &mut value)
                                .unwrap();
                            let _res = unlock_redb(&redb_c, |db| -> Result<()> {
                                let salus_val = SalusVal::builder()
                                    .nonce(*nonce.as_ref())
                                    .ciphertext(value.clone())
                                    .build();
                                match write_value::<String, SalusVal>(
                                    db,
                                    SALUS_VAL_TABLE_DEF,
                                    key_val.to_string(),
                                    salus_val,
                                ) {
                                    Err(e) => {
                                        error!("Error writing value to database: {e}");
                                        return Err(e);
                                    }
                                    Ok(()) => {
                                        info!("Stored value under key: {key_val}");
                                    }
                                }
                                Ok(())
                            });
                        } else {
                            error!("No key available to store value");
                        }
                    });
                }
            }
        }
    });

    // Set up our loop boilerplate that processes our incoming connections.
    loop {
        let conn = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                error!("There was an error with an incoming connection: {e}");
                continue;
            }
        };

        let (mut receiver, mut sender) = conn.split();
        let (tx, mut rx) = unbounded_channel::<Action>();
        let stx_c = stx.clone();
        let redb_c = redb.clone();

        let _client_recv_handle = spawn(async move {
            while let Some(message) = rx.recv().await {
                if let Err(e) = handler(&mut sender, message, stx_c.clone(), redb_c.clone()).await {
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

fn unlock_store(store: &Arc<Mutex<ShareStore>>, store_fn: impl Fn(&mut ShareStore)) {
    let mut share_store = match store.lock() {
        Ok(share_store) => share_store,
        Err(poisoned) => poisoned.into_inner(),
    };
    store_fn(&mut share_store);
}
