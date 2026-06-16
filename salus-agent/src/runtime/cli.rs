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
    /// Enable logging to stdout/stderr in addition to the tracing output file
    /// * NOTE * - This should not be used when running as a service
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
    /// Override the agent IPC socket path (otherwise the shared
    /// `SALUS_AGENT_SOCKET` env var or the platform default is used)
    #[clap(short, long, help = "Specify the path to the agent IPC socket")]
    socket_path: Option<String>,
}

impl Source for Cli {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
        let mut map = Map::new();
        let origin = String::from("command line");
        // Only emit flags the user actually set, so CLI defaults do not clobber
        // values from the lower-precedence env/file sources.
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
        // The `*_absolute_path` overrides are consumed through `PathDefaults`,
        // not the config struct. The socket path lives in the config so it can
        // be layered from file/env/CLI.
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
        // CARGO_PKG_NAME is "salus-agent"; the env prefix uses no hyphen so the
        // variables read `SALUSAGENT_*`.
        env!("CARGO_PKG_NAME").replace('-', "").to_ascii_uppercase()
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
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use clap::Parser;
    use config::Source;

    use super::Cli;
    use crate::config::PathDefaults;

    #[test]
    fn collect_omits_unset_flags() -> Result<()> {
        let cli = Cli::try_parse_from(["salus-agent"])?;
        let map = cli.collect()?;
        assert!(
            map.is_empty(),
            "default Cli should emit nothing, got {map:?}"
        );
        Ok(())
    }

    #[test]
    fn collect_includes_set_flags() -> Result<()> {
        let cli = Cli::try_parse_from(["salus-agent", "-vv", "-e", "-s", "/tmp/a.sock"])?;
        let map = cli.collect()?;
        assert!(map.contains_key("verbose"));
        assert!(map.contains_key("enable_std_output"));
        assert!(map.contains_key("socket_path"));
        assert!(!map.contains_key("quiet"));
        Ok(())
    }

    #[test]
    fn env_prefix_strips_hyphen() -> Result<()> {
        let cli = Cli::try_parse_from(["salus-agent"])?;
        assert_eq!(cli.env_prefix(), "SALUSAGENT");
        Ok(())
    }

    #[test]
    fn collect_includes_quiet() -> Result<()> {
        let cli = Cli::try_parse_from(["salus-agent", "-qq"])?;
        let map = cli.collect()?;
        assert!(map.contains_key("quiet"));
        assert!(!map.contains_key("verbose"));
        Ok(())
    }

    #[test]
    fn path_defaults_expose_overrides() -> Result<()> {
        let cli =
            Cli::try_parse_from(["salus-agent", "-c", "/tmp/cfg.toml", "-t", "/tmp/trace.log"])?;
        assert_eq!(cli.app_name(), "salus-agent");
        assert_eq!(cli.config_absolute_path().as_deref(), Some("/tmp/cfg.toml"));
        assert_eq!(
            cli.tracing_absolute_path().as_deref(),
            Some("/tmp/trace.log")
        );
        Ok(())
    }

    #[test]
    fn path_defaults_default_to_none() -> Result<()> {
        let cli = Cli::try_parse_from(["salus-agent"])?;
        assert!(cli.config_absolute_path().is_none());
        assert!(cli.tracing_absolute_path().is_none());
        Ok(())
    }
}
