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
use serde::{Deserialize, Serialize};

/// The application name, used as the env prefix, per-user directory, and file
/// stem for the client's configuration.
const APP_NAME: &str = env!("CARGO_PKG_NAME");

/// The client configuration, layered (lowest to highest precedence) from a TOML
/// file, `SALUSC_` environment variables, and explicitly-set CLI flags.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct ConfigSalusc {
    /// Optional override for the daemon IPC socket path. Falls back to the shared
    /// `SALUS_SOCKET` env var and then the platform default in libsalus.
    socket_path: Option<String>,
    /// Optional override for the `salus-agent` IPC socket path. Falls back to the
    /// shared `SALUS_AGENT_SOCKET` env var and then the platform default.
    agent_socket_path: Option<String>,
    /// Optional maximum bytes to read from stdin for the `store` subcommand.
    /// When `None`, the default of 65536 (64 KiB) is used. Can be overridden
    /// per-invocation with the `--max-value-bytes` flag.
    store_max_value_bytes: Option<usize>,
}

impl ConfigSalusc {
    pub(crate) fn socket_path(&self) -> Option<&str> {
        self.socket_path.as_deref()
    }

    pub(crate) fn agent_socket_path(&self) -> Option<&str> {
        self.agent_socket_path.as_deref()
    }

    pub(crate) fn store_max_value_bytes(&self) -> Option<usize> {
        self.store_max_value_bytes
    }
}

/// Load the client configuration.
///
/// `config_absolute_path`, when `Some`, is an explicit config file path (from
/// the `--config-path` flag) used instead of the per-user default.
///
/// # Errors
///
/// * Returns an error if no valid config directory can be found, or if the
///   configuration cannot be built or deserialized.
pub(crate) fn load<S>(cli: &S, config_absolute_path: Option<&str>) -> Result<ConfigSalusc>
where
    S: Source + Clone + Send + Sync + 'static,
{
    let config_file_path = config_file_path(config_absolute_path)?;
    let config = Config::builder()
        // Lowest precedence first; the `config` crate is last-wins.
        .add_source(
            File::from(config_file_path)
                .format(FileFormat::Toml)
                .required(false),
        )
        .add_source(env_source(&APP_NAME.to_ascii_uppercase()))
        .add_source(cli.clone())
        .build()
        .context("unable to build salusc configuration")?;
    config
        .try_deserialize()
        .context("unable to deserialize salusc configuration")
}

/// Build the environment-variable config source for `prefix`.
///
/// `prefix_separator("_")` separates the prefix from the key, while
/// `separator("__")` is reserved for nesting, keeping underscore-containing
/// field names intact.
fn env_source(prefix: &str) -> Environment {
    Environment::with_prefix(prefix)
        .prefix_separator("_")
        .separator("__")
        .try_parsing(true)
}

fn config_file_path(config_absolute_path: Option<&str>) -> Result<PathBuf> {
    config_absolute_path.map_or_else(
        || {
            let base = dirs2::config_dir().context("there is no valid config directory")?;
            Ok(config_file_in(&base, APP_NAME))
        },
        |path| Ok(PathBuf::from(path)),
    )
}

/// Compose the default config file path: `<base>/<app>/<app>.toml`.
fn config_file_in(base: &Path, app: &str) -> PathBuf {
    base.join(app).join(app).with_extension("toml")
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use anyhow::Result;
    use config::{Config, Map};

    use super::{ConfigSalusc, config_file_in, env_source};

    #[test]
    fn config_file_in_composes_app_dir_and_extension() {
        let path = config_file_in(Path::new("/base"), "salusc");
        assert_eq!(path, Path::new("/base/salusc/salusc.toml"));
    }

    #[test]
    fn socket_path_from_env() -> Result<()> {
        let mut env = Map::new();
        let _old = env.insert(
            "SALUSC_SOCKET_PATH".to_string(),
            "/tmp/env.sock".to_string(),
        );
        let config = Config::builder()
            .add_source(env_source("SALUSC").source(Some(env)))
            .build()?;
        let cfg: ConfigSalusc = config.try_deserialize()?;
        assert_eq!(cfg.socket_path(), Some("/tmp/env.sock"));
        Ok(())
    }

    #[test]
    fn missing_socket_path_defaults_to_none() -> Result<()> {
        let config = Config::builder().build()?;
        let cfg: ConfigSalusc = config.try_deserialize()?;
        assert!(cfg.socket_path().is_none());
        Ok(())
    }
}
