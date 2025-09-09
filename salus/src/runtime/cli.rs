// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use clap::{ArgAction, Parser, Subcommand};
use config::{ConfigError, Map, Source, Value, ValueKind};

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Set logging verbosity.  More v's, more verbose.
    #[clap(
        short,
        long,
        action = ArgAction::Count,
        help = "Turn up logging verbosity (multiple will turn it up more)",
        conflicts_with = "quiet"
    )]
    verbose: u8,
    /// Set logging quietness.  More q's, more quiet.
    #[clap(
        short,
        long,
        action = ArgAction::Count,
        help = "Turn down logging verbosity (multiple will turn it down more)",
        conflicts_with = "verbose"
    )]
    quiet: u8,
    /// Config file path
    #[clap(short, long, help = "Specify a path to the config file")]
    config_path: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    pub(crate) fn command(self) -> Commands {
        self.command
    }
}

impl Source for Cli {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
        let mut map = Map::new();
        let origin = String::from("command line");
        let _old = map.insert(
            "verbose".to_string(),
            Value::new(Some(&origin), ValueKind::U64(u8::into(self.verbose))),
        );
        let _old = map.insert(
            "quiet".to_string(),
            Value::new(Some(&origin), ValueKind::U64(u8::into(self.quiet))),
        );
        if let Some(config_path) = &self.config_path {
            let _old = map.insert(
                "config_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(config_path.clone())),
            );
        }
        Ok(map)
    }
}

#[derive(Clone, Copy, Debug, Subcommand)]
pub(crate) enum Commands {
    Genkey,
    Init {
        /// The number of shares to create
        #[arg(short, long, default_value = "5")]
        num_shares: u8,
        /// The number of shares required to reconstruct the secret
        #[arg(short, long, default_value = "3")]
        threshold: u8,
    },
    Unlock,
}
