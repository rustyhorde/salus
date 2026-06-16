// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use config::{Config, Environment, File, FileFormat, Source};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};
use tracing_subscriber_init::TracingConfig;

use crate::{error::Error, utils::to_path_buf};

/// Trait to allow default paths to be supplied to [`load`].
///
/// Default locations are derived per-platform from `dirs2` using
/// [`app_name`](PathDefaults::app_name); the `*_absolute_path` methods provide
/// explicit overrides (e.g. from a CLI flag) that bypass those defaults.
pub(crate) trait PathDefaults {
    /// Environment variable prefix
    fn env_prefix(&self) -> String;
    /// The application name used as the per-user directory and file stem
    fn app_name(&self) -> String;
    /// The absolute path to use for the config file
    fn config_absolute_path(&self) -> Option<String>;
    /// The absolute path to use for tracing output
    fn tracing_absolute_path(&self) -> Option<String>;
}

/// The documented default for [`ConfigSalusAgent::passphrase_cache_timeout`].
///
/// The agent caches an unsealed final share for this many seconds after a
/// successful unseal so the passphrase is typed once per session rather than on
/// every `unlock`. Set to `0` to require the passphrase on every unlock.
const DEFAULT_PASSPHRASE_CACHE_TIMEOUT: u64 = 3600;

// `#[serde(default)]` fills any field absent from all config sources from
// `Default`, making the built-in defaults the lowest-precedence layer.
#[derive(Clone, CopyGetters, Debug, Deserialize, Eq, Getters, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct ConfigSalusAgent {
    #[getset(get_copy = "pub(crate)")]
    verbose: u8,
    #[getset(get_copy = "pub(crate)")]
    quiet: u8,
    #[getset(get_copy = "pub(crate)")]
    enable_std_output: bool,
    /// How long, in seconds, the agent keeps an unsealed final share cached.
    #[getset(get_copy = "pub(crate)")]
    passphrase_cache_timeout: u64,
    /// Optional override for the agent IPC socket path. Falls back to the shared
    /// `SALUS_AGENT_SOCKET` env var and then the platform default in libsalus.
    #[getset(get = "pub(crate)")]
    socket_path: Option<String>,
    #[getset(get = "pub(crate)")]
    tracing: Tracing,
}

impl Default for ConfigSalusAgent {
    fn default() -> Self {
        Self {
            verbose: 0,
            quiet: 0,
            enable_std_output: false,
            passphrase_cache_timeout: DEFAULT_PASSPHRASE_CACHE_TIMEOUT,
            socket_path: None,
            tracing: Tracing::default(),
        }
    }
}

impl TracingConfig for ConfigSalusAgent {
    fn quiet(&self) -> u8 {
        self.quiet
    }

    fn verbose(&self) -> u8 {
        self.verbose
    }

    fn with_target(&self) -> bool {
        self.tracing.with_target
    }

    fn with_thread_ids(&self) -> bool {
        self.tracing.with_thread_ids
    }

    fn with_thread_names(&self) -> bool {
        self.tracing.with_thread_names
    }

    fn with_line_number(&self) -> bool {
        self.tracing.with_line_number
    }

    fn with_level(&self) -> bool {
        self.tracing.with_level
    }
}

/// Tracing configuration
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, CopyGetters, Debug, Default, Deserialize, Eq, Getters, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct Tracing {
    /// Should we trace the event target
    #[getset(get_copy = "pub(crate)")]
    with_target: bool,
    /// Should we trace the thread id
    #[getset(get_copy = "pub(crate)")]
    with_thread_ids: bool,
    /// Should we trace the thread names
    #[getset(get_copy = "pub(crate)")]
    with_thread_names: bool,
    /// Should we trace the line numbers
    #[getset(get_copy = "pub(crate)")]
    with_line_number: bool,
    /// Should we trace the level
    #[getset(get_copy = "pub(crate)")]
    with_level: bool,
    /// Additional tracing directives
    #[getset(get = "pub(crate)")]
    directives: Option<String>,
}

/// Load the configuration
pub(crate) fn load<'a, S, T, D>(cli: &S, defaults: &D) -> Result<T>
where
    T: Deserialize<'a>,
    S: Source + Clone + Send + Sync + 'static,
    D: PathDefaults,
{
    let config_file_path = config_file_path(defaults)?;
    let config = Config::builder()
        // Lowest precedence first; the `config` crate is last-wins, so the order
        // is: TOML file -> environment -> explicitly-set CLI flags.
        .add_source(
            File::from(config_file_path)
                .format(FileFormat::Toml)
                .required(false),
        )
        .add_source(env_source(&defaults.env_prefix()))
        .add_source(cli.clone())
        .build()
        .with_context(|| Error::ConfigBuild)?;
    config
        .try_deserialize::<T>()
        .with_context(|| Error::ConfigDeserialize)
}

/// Build the environment-variable config source for `prefix`.
///
/// `prefix_separator("_")` separates the prefix from the key, while
/// `separator("__")` is used for nesting. This keeps field names that contain
/// underscores intact (`SALUSAGENT_PASSPHRASE_CACHE_TIMEOUT` ->
/// `passphrase_cache_timeout`) and reserves the double underscore for descending
/// into nested structs (`SALUSAGENT_TRACING__WITH_TARGET` ->
/// `tracing.with_target`).
pub(crate) fn env_source(prefix: &str) -> Environment {
    Environment::with_prefix(prefix)
        .prefix_separator("_")
        .separator("__")
        .try_parsing(true)
}

fn config_file_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let default_fn = || -> Result<PathBuf> { default_config_file_path(defaults) };
    defaults
        .config_absolute_path()
        .as_ref()
        .map_or_else(default_fn, to_path_buf)
}

fn default_config_file_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let base = dirs2::config_dir().ok_or(Error::ConfigDir)?;
    Ok(config_file_in(&base, &defaults.app_name()))
}

/// Compose the default config file path: `<base>/<app>/<app>.toml`.
fn config_file_in(base: &Path, app: &str) -> PathBuf {
    base.join(app).join(app).with_extension("toml")
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use anyhow::Result;
    use config::{Config, ConfigError, Map, Source, Value, ValueKind};

    use super::{
        ConfigSalusAgent, DEFAULT_PASSPHRASE_CACHE_TIMEOUT, PathDefaults, Tracing, config_file_in,
        env_source, load,
    };

    /// A test double that is both a CLI [`Source`] and a [`PathDefaults`], so a
    /// single value drives `load` end-to-end without the real `runtime::cli::Cli`.
    #[derive(Clone, Debug)]
    struct TestCli {
        config_path: Option<String>,
        socket_path: Option<String>,
    }

    impl Source for TestCli {
        fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
            Box::new(self.clone())
        }

        fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
            let mut map = Map::new();
            if let Some(socket_path) = &self.socket_path {
                let _old = map.insert(
                    "socket_path".to_string(),
                    Value::new(None, ValueKind::String(socket_path.clone())),
                );
            }
            Ok(map)
        }
    }

    impl PathDefaults for TestCli {
        // A prefix no real environment uses, so `load` cannot pick up stray vars.
        fn env_prefix(&self) -> String {
            "SALUSAGENTTEST".to_string()
        }
        fn app_name(&self) -> String {
            "salus-agent-test".to_string()
        }
        fn config_absolute_path(&self) -> Option<String> {
            self.config_path.clone()
        }
        fn tracing_absolute_path(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn config_file_in_composes_app_dir_and_extension() {
        let path = config_file_in(Path::new("/base"), "salus-agent");
        assert_eq!(path, Path::new("/base/salus-agent/salus-agent.toml"));
    }

    #[test]
    fn defaults_match_documented_values() {
        let cfg = ConfigSalusAgent::default();
        assert_eq!(cfg.verbose(), 0);
        assert_eq!(cfg.quiet(), 0);
        assert!(!cfg.enable_std_output());
        assert_eq!(
            cfg.passphrase_cache_timeout(),
            DEFAULT_PASSPHRASE_CACHE_TIMEOUT
        );
        assert!(cfg.socket_path().is_none());

        let tracing = Tracing::default();
        assert!(!tracing.with_target());
        assert!(!tracing.with_level());
        assert!(tracing.directives().is_none());
    }

    #[test]
    fn load_layers_file_env_and_cli() -> Result<()> {
        // Point at a non-existent config file so the (optional) file source is
        // skipped, then let the CLI source supply an explicitly-set flag.
        let cli = TestCli {
            config_path: Some("/nonexistent/salus-agent-test.toml".to_string()),
            socket_path: Some("/tmp/agent-test.sock".to_string()),
        };
        let cfg: ConfigSalusAgent = load(&cli, &cli)?;
        // The CLI socket override wins; everything else falls back to defaults.
        assert_eq!(cfg.socket_path().as_deref(), Some("/tmp/agent-test.sock"));
        assert_eq!(
            cfg.passphrase_cache_timeout(),
            DEFAULT_PASSPHRASE_CACHE_TIMEOUT
        );
        Ok(())
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() -> Result<()> {
        let config = Config::builder().build()?;
        let cfg: ConfigSalusAgent = config.try_deserialize()?;
        assert_eq!(
            cfg.passphrase_cache_timeout(),
            DEFAULT_PASSPHRASE_CACHE_TIMEOUT
        );
        assert_eq!(cfg.verbose(), 0);
        assert!(!cfg.enable_std_output());
        assert!(cfg.socket_path().is_none());
        Ok(())
    }

    #[test]
    fn env_separators_map_flat_and_nested_fields() -> Result<()> {
        let mut map = Map::new();
        let _old = map.insert(
            "SALUSAGENT_PASSPHRASE_CACHE_TIMEOUT".to_string(),
            "99".to_string(),
        );
        let _old = map.insert(
            "SALUSAGENT_TRACING__WITH_TARGET".to_string(),
            "true".to_string(),
        );
        let config = Config::builder()
            .add_source(env_source("SALUSAGENT").source(Some(map)))
            .build()?;
        let cfg: ConfigSalusAgent = config.try_deserialize()?;
        assert_eq!(cfg.passphrase_cache_timeout(), 99);
        assert!(cfg.tracing().with_target());
        Ok(())
    }
}
