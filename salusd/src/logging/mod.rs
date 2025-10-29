// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{fs::File, path::PathBuf};

use anyhow::Result;
use tracing::{Level, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, Layer, Registry, fmt::time::UtcTime};
use tracing_subscriber_init::{Iso8601, TracingConfig, compact, try_init};

use crate::{
    config::{ConfigSalusd, PathDefaults},
    utils::to_path_buf,
};

/// Initialize tracing
pub(crate) fn initialize<T, U>(
    tracing_config: &T,
    config: &ConfigSalusd,
    defaults: &U,
    layers_opt: Option<Vec<Box<dyn Layer<Registry> + Send + Sync>>>,
) -> Result<()>
where
    T: TracingConfig,
    U: PathDefaults,
{
    let mut layers = layers_opt.unwrap_or_default();

    // Setup the stdout tracing layer if enabled
    if config.enable_std_output() {
        let (layer, level_filter) = compact(tracing_config);
        let directives = directives(config, level_filter);
        let filter = EnvFilter::builder()
            .with_default_directive(level_filter.into())
            .parse_lossy(directives);
        let stdout_layer = layer
            .with_timer(UtcTime::new(Iso8601::DEFAULT))
            .with_filter(filter);
        layers.push(stdout_layer.boxed());
    }

    // Setup the tracing file layer
    let tracing_absolute_path = tracing_absolute_path(defaults)?;
    let tracing_file = File::create(&tracing_absolute_path)?;
    let (layer, level_filter) = compact(tracing_config);
    let directives = directives(config, level_filter);
    let filter = EnvFilter::builder()
        .with_default_directive(level_filter.into())
        .parse_lossy(directives);
    let file_layer = layer
        .with_timer(UtcTime::new(Iso8601::DEFAULT))
        .with_writer(tracing_file)
        .with_filter(filter);
    layers.push(file_layer.boxed());

    try_init(layers)?;
    Ok(())
}

fn directives(config: &ConfigSalusd, level_filter: LevelFilter) -> String {
    let directives_base = match level_filter.into_level() {
        Some(level) => match level {
            Level::TRACE => "trace",
            Level::DEBUG => "debug",
            Level::INFO => "info",
            Level::WARN => "warn",
            Level::ERROR => "error",
        },
        None => "info",
    };

    if let Some(directives) = config.tracing().directives() {
        format!("{directives_base},{directives}")
    } else {
        directives_base.to_string()
    }
}

fn tracing_absolute_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let default_fn = || -> Result<PathBuf> { default_tracing_absolute_path(defaults) };
    defaults
        .tracing_absolute_path()
        .as_ref()
        .map_or_else(default_fn, to_path_buf)
}

#[allow(clippy::unnecessary_wraps)]
fn default_tracing_absolute_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let mut config_file_path = PathBuf::from(defaults.default_tracing_path());
    config_file_path.push(defaults.default_tracing_file_name());
    let _ = config_file_path.set_extension("log");
    Ok(config_file_path)
}
