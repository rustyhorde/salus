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
        let _old = map.insert(
            "enable_std_output".to_string(),
            Value::new(Some(&origin), ValueKind::Boolean(self.enable_std_output)),
        );
        if let Some(config_path) = &self.config_absolute_path {
            let _old = map.insert(
                "config_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(config_path.clone())),
            );
        }
        if let Some(tracing_path) = &self.tracing_absolute_path {
            let _old = map.insert(
                "tracing_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(tracing_path.clone())),
            );
        }
        if let Some(database_path) = &self.database_absolute_path {
            let _old = map.insert(
                "database_path".to_string(),
                Value::new(Some(&origin), ValueKind::String(database_path.clone())),
            );
        }
        Ok(map)
    }
}

impl PathDefaults for Cli {
    fn env_prefix(&self) -> String {
        env!("CARGO_PKG_NAME").to_ascii_uppercase()
    }

    fn config_absolute_path(&self) -> Option<String> {
        self.config_absolute_path.clone()
    }

    fn default_file_path(&self) -> String {
        format!("/var/lib/{}", env!("CARGO_PKG_NAME"))
    }

    fn default_file_name(&self) -> String {
        env!("CARGO_PKG_NAME").to_string()
    }

    fn tracing_absolute_path(&self) -> Option<String> {
        self.tracing_absolute_path.clone()
    }

    fn default_tracing_path(&self) -> String {
        format!("/var/log/{}", env!("CARGO_PKG_NAME"))
    }

    fn default_tracing_file_name(&self) -> String {
        env!("CARGO_PKG_NAME").to_string()
    }

    fn database_absolute_path(&self) -> Option<String> {
        self.database_absolute_path.clone()
    }

    fn default_database_path(&self) -> String {
        format!("/var/lib/{}", env!("CARGO_PKG_NAME"))
    }

    fn default_database_file_name(&self) -> String {
        env!("CARGO_PKG_NAME").to_string()
    }
}
