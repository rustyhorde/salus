// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use config::{ConfigError, Map, Source, Value, ValueKind};

/// Command-line client for the salus secret store.
///
/// salusc talks to the `salusd` daemon over a local IPC socket. Initialize the
/// store with `shares`, reconstruct the key with `unlock`, then `store`, `read`,
/// `find`, and `delete` secrets. When the `salus-agent` is enrolled it can
/// supply the unlock shares for you (see `enroll`). The client holds no key
/// material and performs no cryptography itself.
#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about)]
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
    /// Override the salus-agent IPC socket path (otherwise the shared
    /// `SALUS_AGENT_SOCKET` env var or the platform default is used)
    #[clap(
        short = 'a',
        long,
        help = "Specify the path to the salus-agent IPC socket"
    )]
    agent_socket_path: Option<String>,
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
        if let Some(agent_socket_path) = &self.agent_socket_path {
            let _old = map.insert(
                "agent_socket_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(agent_socket_path.clone())),
            );
        }
        Ok(map)
    }
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum Commands {
    /// Initialize the store and print its key shares (first-time setup, once)
    ///
    /// Generates a fresh master key, splits it into Shamir shares, and prints
    /// them a single time. Record them somewhere safe — they are required to
    /// `unlock` the store and are never shown again.
    Shares {
        /// The number of shares to create
        #[arg(short, long, default_value = "5", value_name = "COUNT")]
        num_shares: u8,
        /// The number of shares required to reconstruct the key
        #[arg(short, long, default_value = "3", value_name = "COUNT")]
        threshold: u8,
    },
    /// Reconstruct the key in the daemon's memory from `threshold` shares
    ///
    /// Prompts for the required shares (or has the agent supply them when a set
    /// is enrolled), then holds the key for the unlock duration before it
    /// auto-clears.
    Unlock {
        /// The named enrollment set to unlock with (when the agent is enrolled).
        /// Omit to use the only set, or to be prompted when several exist.
        #[arg(short, long, value_name = "NAME")]
        set: Option<String>,
        /// How long the daemon should hold the key: a number of seconds (capped
        /// at 24h), or "forever". Omit to use the daemon's configured default.
        #[arg(short = 'f', long = "for", value_name = "SECONDS|forever")]
        duration: Option<String>,
    },
    /// Clear the daemon's unlocked key and cancel any pending auto-clear timer
    Lock,
    /// Encrypt and store a value under a key
    ///
    /// Provide the value as the second argument, or omit it to read the value
    /// from stdin (e.g. `echo secret | salusc store mykey`). The store must be
    /// unlocked first. If the key already exists, prompts for confirmation
    /// before overwriting unless `--force` is given.
    Store {
        /// The key to store the value under
        #[arg(value_name = "KEY")]
        key: String,
        /// The value to store; if omitted, it is read from stdin
        #[arg(value_name = "VALUE")]
        value: Option<String>,
        /// Maximum bytes to read from stdin (default: 65536)
        #[arg(long, value_name = "BYTES")]
        max_value_bytes: Option<usize>,
        /// Overwrite an existing value without prompting for confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Read and decrypt the value stored under a key
    ///
    /// The store must be unlocked first.
    Read {
        /// The key to read the value from
        #[arg(value_name = "KEY")]
        key: String,
    },
    /// Permanently delete the value stored under a key
    ///
    /// Prompts for confirmation unless `--force` is given. The store must be
    /// unlocked first.
    Delete {
        /// The key to delete from the store
        #[arg(value_name = "KEY")]
        key: String,
        /// Skip the confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
    /// Search stored keys by regular expression
    ///
    /// The store must be unlocked first.
    Find {
        /// The regex to match key names against
        #[arg(index = 1, value_name = "REGEX")]
        regex: String,
    },
    /// Predictively (fuzzy) search stored key names
    ///
    /// Omit QUERY to open an interactive filter prompt: type to narrow the
    /// list, Up/Down to move, Enter to print the selected key's value, Esc to
    /// cancel.
    /// The store must be unlocked first.
    Search {
        /// The query to fuzzy-match against key names
        #[arg(index = 1, value_name = "QUERY")]
        query: Option<String>,
        /// Maximum number of results to show
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Enroll a named set of shares so the agent can supply them at unlock
    Enroll {
        /// The name of the enrollment set
        #[arg(short, long, default_value = "default")]
        name: String,
        /// Replace an existing set with the same name
        #[arg(long)]
        force: bool,
        /// Store this set's automatic shares separately instead of reusing the
        /// shared ones (accepts the documented keyring-union risk)
        #[arg(long)]
        independent_auto: bool,
    },
    /// Remove a named enrolled set, or every set with --all
    ///
    /// Prompts for confirmation unless `--force` is given.
    Forget {
        /// The name of the set to remove
        #[arg(short, long, conflicts_with = "all")]
        name: Option<String>,
        /// Remove every enrolled set
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
    /// List the enrolled sets and whether the agent is reachable
    EnrollStatus,
    /// Generate a random password or passphrase
    ///
    /// By default produces a 30-character password drawn from lowercase letters
    /// plus (unless disabled) uppercase letters, digits, and symbols, with at
    /// least one character from each enabled class. Use `--passphrase N` to
    /// generate an N-word passphrase instead; `--passphrase` and `--kind`
    /// cannot be combined with the character-class flags (`-l`/`-c`/`-n`/`-s`).
    /// Pass `-k/--key` to also store the generated value under that key (the
    /// store must be unlocked).
    Gen {
        /// Password length (8-1024)
        #[arg(
            short,
            long,
            default_value_t = 30,
            value_parser = clap::value_parser!(u32).range(8..=1024),
            value_name = "N"
        )]
        length: u32,
        /// Include uppercase letters (pass `-c false` to disable)
        #[arg(
            short,
            long,
            action = ArgAction::Set,
            num_args = 0..=1,
            default_value_t = true,
            default_missing_value = "true",
            value_name = "BOOL"
        )]
        caps: bool,
        /// Include digits (pass `-n false` to disable)
        #[arg(
            short,
            long,
            action = ArgAction::Set,
            num_args = 0..=1,
            default_value_t = true,
            default_missing_value = "true",
            value_name = "BOOL"
        )]
        numbers: bool,
        /// Include symbols (pass `-s false` to disable)
        #[arg(
            short,
            long,
            action = ArgAction::Set,
            num_args = 0..=1,
            default_value_t = true,
            default_missing_value = "true",
            value_name = "BOOL"
        )]
        special: bool,
        /// Generate an N-word passphrase instead of a character password (1-20)
        #[arg(
            long,
            value_parser = clap::value_parser!(u32).range(1..=20),
            value_name = "N",
            conflicts_with_all = ["length", "caps", "numbers", "special"]
        )]
        passphrase: Option<u32>,
        /// Passphrase word formatting (only meaningful with --passphrase)
        #[arg(
            long,
            value_enum,
            default_value_t = GenKind::Space,
            conflicts_with_all = ["length", "caps", "numbers", "special"]
        )]
        kind: GenKind,
        /// Also store the generated value under this key (store must be unlocked)
        #[arg(short, long, value_name = "KEY")]
        key: Option<String>,
    },
}

/// How the words of a generated passphrase are joined together.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum GenKind {
    /// Space-separated lowercase words: `correct horse battery staple`
    Space,
    /// Hyphen-separated lowercase words: `correct-horse-battery-staple`
    Hyphen,
    /// Dot-separated lowercase words: `correct.horse.battery.staple`
    Dot,
    /// Capitalized words with no separator: `CorrectHorseBatteryStaple`
    Camel,
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use clap::Parser;
    use config::Source;

    use super::Cli;

    #[test]
    fn collect_omits_unset_flags() -> Result<()> {
        let cli = Cli::try_parse_from(["salusc", "unlock"])?;
        let map = cli.collect()?;
        assert!(
            map.is_empty(),
            "default Cli should emit nothing, got {map:?}"
        );
        Ok(())
    }

    #[test]
    fn collect_includes_set_socket_path() -> Result<()> {
        let cli = Cli::try_parse_from(["salusc", "-s", "/tmp/s.sock", "unlock"])?;
        let map = cli.collect()?;
        assert!(map.contains_key("socket_path"));
        assert!(!map.contains_key("verbose"));
        Ok(())
    }

    #[test]
    fn collect_includes_agent_socket_path() -> Result<()> {
        let cli = Cli::try_parse_from(["salusc", "-a", "/tmp/a.sock", "unlock"])?;
        let map = cli.collect()?;
        assert!(map.contains_key("agent_socket_path"));
        assert!(!map.contains_key("socket_path"));
        Ok(())
    }
}
