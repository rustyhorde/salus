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
    /// Override the IPC socket path (otherwise the shared `SALUS_SOCKET` env var
    /// or the platform default is used)
    #[clap(short, long, help = "Specify the path to the IPC socket")]
    socket_path: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    pub(crate) fn command(self) -> Commands {
        self.command
    }

    pub(crate) fn config_path(&self) -> Option<&str> {
        self.config_path.as_deref()
    }
}

impl Source for Cli {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
        let mut map = Map::new();
        let origin = String::from("command line");
        // Only emit flags the user actually set, so CLI defaults do not clobber
        // values from the lower-precedence env/file sources. The `config_path`
        // override is consumed directly to locate the file, not layered here.
        if self.verbose > 0 {
            let _old = map.insert(
                "verbose".to_string(),
                Value::new(Some(&origin), ValueKind::U64(u8::into(self.verbose))),
            );
        }
        if self.quiet > 0 {
            let _old = map.insert(
                "quiet".to_string(),
                Value::new(Some(&origin), ValueKind::U64(u8::into(self.quiet))),
            );
        }
        if let Some(socket_path) = &self.socket_path {
            let _old = map.insert(
                "socket_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(socket_path.clone())),
            );
        }
        Ok(map)
    }
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum Commands {
    Shares {
        /// The number of shares to create
        #[arg(short, long, default_value = "5")]
        num_shares: u8,
        /// The number of shares required to reconstruct the secret
        #[arg(short, long, default_value = "3")]
        threshold: u8,
    },
    Unlock,
    Store {
        /// The key to store the value under
        #[arg(short, long)]
        key: String,
        /// The value to store
        #[arg(short, long)]
        value: String,
    },
    Read {
        /// The key to read the value from
        #[arg(short, long)]
        key_opt: Option<String>,
    },
    Find {
        /// The regex to find keys with
        #[arg(index = 1)]
        regex: String,
    },
}

#[cfg(test)]
mod test {
    use clap::Parser;
    use config::Source;

    use super::Cli;

    #[test]
    fn collect_omits_unset_flags() {
        let cli = Cli::try_parse_from(["salusc", "unlock"]).unwrap();
        let map = cli.collect().unwrap();
        assert!(
            map.is_empty(),
            "default Cli should emit nothing, got {map:?}"
        );
    }

    #[test]
    fn collect_includes_set_socket_path() {
        let cli = Cli::try_parse_from(["salusc", "-s", "/tmp/s.sock", "unlock"]).unwrap();
        let map = cli.collect().unwrap();
        assert!(map.contains_key("socket_path"));
        assert!(!map.contains_key("verbose"));
    }
}
