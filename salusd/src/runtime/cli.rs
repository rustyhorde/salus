// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use clap::{ArgAction, Parser};
use config::{ConfigError, Map, Source, Value, ValueKind};
use getset::Getters;

use crate::config::PathDefaults;

#[derive(Clone, Debug, Getters, Parser)]
#[command(author, version, about, long_about = None)]
#[getset(get = "pub(crate)")]
pub(crate) struct Cli {
    /// Set logging verbosity.  More v's, more verbose.
    #[clap(
        short,
        long,
        action = ArgAction::Count,
        help = "Turn up logging verbosity (multiple will turn it up more)",
        conflicts_with = "quiet",
    )]
    verbose: u8,
    /// Set logging quietness.  More q's, more quiet.
    #[clap(
        short,
        long,
        action = ArgAction::Count,
        help = "Turn down logging verbosity (multiple will turn it down more)",
        conflicts_with = "verbose",
    )]
    quiet: u8,
    /// Enable logging to stdout/stderr in additions to the tracing output file
    /// * NOTE * - This should not be used when running as a daemon/service
    #[clap(short, long, help = "Enable logging to stdout/stderr")]
    enable_std_output: bool,
    /// The absolute path to a non-standard config file
    #[clap(short, long, help = "Specify the absolute path to the config file")]
    config_absolute_path: Option<String>,
    /// The absolute path to a non-standard tracing output file
    #[clap(
        short,
        long,
        help = "Specify the absolute path to the tracing output file"
    )]
    tracing_absolute_path: Option<String>,
    /// The absolute path to a non-standard database file
    #[clap(short, long, help = "Specify the absolute path to the database file")]
    database_absolute_path: Option<String>,
    /// Override the IPC socket path (otherwise the shared `SALUS_SOCKET` env var
    /// or the platform default is used)
    #[clap(short, long, help = "Specify the path to the IPC socket")]
    socket_path: Option<String>,
}

impl Source for Cli {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
        let mut map = Map::new();
        let origin = String::from("command line");
        // Only emit flags the user actually set. For `Count`/`bool` flags there
        // is no CLI syntax for the default, so a non-default value is equivalent
        // to "explicitly set". Emitting defaults here would clobber values from
        // the lower-precedence env/file sources.
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
        if self.enable_std_output {
            let _old = map.insert(
                "enable_std_output".to_string(),
                Value::new(Some(&origin), ValueKind::Boolean(true)),
            );
        }
        // The `*_absolute_path` config/tracing/database overrides are consumed
        // directly through `PathDefaults`, not the config struct, so they are
        // intentionally not emitted here. The socket path, however, lives in
        // `ConfigSalusd` so it can be layered from file/env/CLI.
        if let Some(socket_path) = &self.socket_path {
            let _old = map.insert(
                "socket_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(socket_path.clone())),
            );
        }
        Ok(map)
    }
}

impl PathDefaults for Cli {
    fn env_prefix(&self) -> String {
        env!("CARGO_PKG_NAME").to_ascii_uppercase()
    }

    fn app_name(&self) -> String {
        env!("CARGO_PKG_NAME").to_string()
    }

    fn config_absolute_path(&self) -> Option<String> {
        self.config_absolute_path.clone()
    }

    fn tracing_absolute_path(&self) -> Option<String> {
        self.tracing_absolute_path.clone()
    }

    fn database_absolute_path(&self) -> Option<String> {
        self.database_absolute_path.clone()
    }
}

#[cfg(test)]
mod test {
    use clap::Parser;
    use config::{Config, Map, Source};

    use super::Cli;
    use crate::config::{ConfigSalusd, env_source};

    #[test]
    fn collect_omits_unset_flags() {
        let cli = Cli::try_parse_from(["salusd"]).unwrap();
        let map = cli.collect().unwrap();
        assert!(
            map.is_empty(),
            "default Cli should emit nothing, got {map:?}"
        );
    }

    #[test]
    fn collect_includes_set_flags() {
        let cli = Cli::try_parse_from(["salusd", "-vv", "-e", "-s", "/tmp/s.sock"]).unwrap();
        let map = cli.collect().unwrap();
        assert!(map.contains_key("verbose"));
        assert!(map.contains_key("enable_std_output"));
        assert!(map.contains_key("socket_path"));
        assert!(!map.contains_key("quiet"));
    }

    #[test]
    fn cli_default_does_not_clobber_env_verbose() {
        let mut env = Map::new();
        let _old = env.insert("SALUSD_VERBOSE".to_string(), "3".to_string());
        // No `-v` on the command line, so the CLI source must not override env.
        let cli = Cli::try_parse_from(["salusd"]).unwrap();
        let config = Config::builder()
            .add_source(env_source("SALUSD").source(Some(env)))
            .add_source(cli)
            .build()
            .unwrap();
        let cfg: ConfigSalusd = config.try_deserialize().unwrap();
        assert_eq!(cfg.verbose(), 3);
    }

    #[test]
    fn explicit_cli_overrides_env_verbose() {
        let mut env = Map::new();
        let _old = env.insert("SALUSD_VERBOSE".to_string(), "3".to_string());
        let cli = Cli::try_parse_from(["salusd", "-v"]).unwrap();
        let config = Config::builder()
            .add_source(env_source("SALUSD").source(Some(env)))
            .add_source(cli)
            .build()
            .unwrap();
        let cfg: ConfigSalusd = config.try_deserialize().unwrap();
        assert_eq!(cfg.verbose(), 1);
    }
}
