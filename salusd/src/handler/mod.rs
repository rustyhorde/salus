// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use anyhow::Result;
use bincode::{config::standard, encode_to_vec};
use interprocess::local_socket::traits::tokio::SendHalf;
use libsalus::{Action, Response, Shares, SsssConfig, gen_key};
use tokio::{io::AsyncWriteExt, sync::mpsc::UnboundedSender};
use tracing::{error, trace, warn};

pub(crate) enum ShareStoreMessage {
    AddShare(String),
    Unlock,
    ClearKey,
}

#[derive(Default)]
pub(crate) struct ShareStore {
    shares: Vec<String>,
    key: Vec<u8>,
}

impl ShareStore {
    pub(crate) fn clear_shares(&mut self) {
        self.shares.clear();
    }

    pub(crate) fn clear_key(&mut self) {
        self.key.clear();
    }

    pub(crate) fn add_key(&mut self, key: Vec<u8>) {
        self.key = key;
    }

    pub(crate) fn add_share(&mut self, share: String) {
        self.shares.push(share);
    }

    pub(crate) fn share_count(&self) -> usize {
        self.shares.len()
    }

    pub(crate) fn shares(&self) -> Vec<String> {
        self.shares.clone()
    }
}

pub(crate) async fn handler<T: SendHalf + Unpin>(
    sender: &mut T,
    message: Action,
    stx: UnboundedSender<ShareStoreMessage>,
) -> Result<()> {
    trace!("Got a message from a client: {message:?}");
    match message {
        Action::Genkey => {
            if let Ok(shares) = gen_key(&SsssConfig::default()) {
                let shares_msg = Response::Shares(Shares::builder().shares(shares).build());
                response(sender, shares_msg).await?;
            } else {
                error!("Error generating shares");
                error(sender).await?;
            }
        }
        Action::Share(share) => {
            trace!("Share received");
            stx.send(ShareStoreMessage::AddShare(share.share().to_string()))?;
            success(sender).await?;
        }
        Action::Unlock => {
            stx.send(ShareStoreMessage::Unlock)?;
            success(sender).await?;
        }
        Action::Init(_init) => {
            warn!("Initialize requested, but not implemented.");
            success(sender).await?;
        }
    }
    Ok(())
}

async fn response<T: SendHalf + Unpin>(sender: &mut T, message: Response) -> Result<()> {
    let message = encode_to_vec(message, standard())?;
    sender.write_all(&message).await?;
    sender.flush().await?;
    Ok(())
}

async fn success<T: SendHalf + Unpin>(sender: &mut T) -> Result<()> {
    response(sender, Response::Success).await
}

async fn error<T: SendHalf + Unpin>(sender: &mut T) -> Result<()> {
    response(sender, Response::Error).await
}
