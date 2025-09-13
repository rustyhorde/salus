// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::path::PathBuf;

use anyhow::{Context, Result};
use config::{Config, Environment, File, FileFormat, Source};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};
use tracing_subscriber_init::TracingConfig;

use crate::{error::Error, utils::to_path_buf};

/// Trait to allow default paths to be supplied to [`load`]
pub(crate) trait PathDefaults {
    /// Environment variable prefix
    fn env_prefix(&self) -> String;
    /// The absolute path to use for the config file
    fn config_absolute_path(&self) -> Option<String>;
    /// The default file path to use
    fn default_file_path(&self) -> String;
    /// The default file name to use
    fn default_file_name(&self) -> String;
    /// The abolute path to use for tracing output
    fn tracing_absolute_path(&self) -> Option<String>;
    /// The default logging path to use
    fn default_tracing_path(&self) -> String;
    /// The default log file name to use
    fn default_tracing_file_name(&self) -> String;
    /// The absolute path to use for the database
    fn database_absolute_path(&self) -> Option<String>;
    /// The default database path to use
    fn default_database_path(&self) -> String;
    /// The default database file name to use
    fn default_database_file_name(&self) -> String;
}

#[derive(Clone, CopyGetters, Debug, Default, Deserialize, Eq, Getters, PartialEq, Serialize)]
pub(crate) struct ConfigSalusd {
    #[getset(get_copy = "pub(crate)")]
    verbose: u8,
    #[getset(get_copy = "pub(crate)")]
    quiet: u8,
    #[getset(get_copy = "pub(crate)")]
    enable_std_output: bool,
    #[getset(get = "pub(crate)")]
    tracing: Tracing,
}

impl TracingConfig for ConfigSalusd {
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
        .add_source(
            Environment::with_prefix(&defaults.env_prefix())
                .separator("_")
                .try_parsing(true),
        )
        .add_source(cli.clone())
        .add_source(File::from(config_file_path).format(FileFormat::Toml))
        .build()
        .with_context(|| Error::ConfigBuild)?;
    config
        .try_deserialize::<T>()
        .with_context(|| Error::ConfigDeserialize)
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
    let mut config_file_path = dirs2::config_dir().ok_or(Error::ConfigDir)?;
    config_file_path.push(defaults.default_file_path());
    config_file_path.push(defaults.default_file_name());
    let _ = config_file_path.set_extension("toml");
    Ok(config_file_path)
}
