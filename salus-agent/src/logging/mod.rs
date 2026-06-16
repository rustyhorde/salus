// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::Result;
use tracing::{Level, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, Layer, Registry, fmt::time::UtcTime};
use tracing_subscriber_init::{Iso8601, TracingConfig, compact, try_init};

use crate::{
    config::{ConfigSalusAgent, PathDefaults},
    error::Error,
    utils::{ensure_parent_dir, to_path_buf},
};

/// Initialize tracing
pub(crate) fn initialize<T, U>(
    tracing_config: &T,
    config: &ConfigSalusAgent,
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
            .with_ansi(true)
            .with_ansi_sanitization(false)
            .with_timer(UtcTime::new(Iso8601::DEFAULT))
            .with_filter(filter);
        layers.push(stdout_layer.boxed());
    }

    // Setup the tracing file layer
    let tracing_absolute_path = tracing_absolute_path(defaults)?;
    ensure_parent_dir(&tracing_absolute_path)?;
    let tracing_file = File::create(&tracing_absolute_path)?;
    let (layer, level_filter) = compact(tracing_config);
    let directives = directives(config, level_filter);
    let filter = EnvFilter::builder()
        .with_default_directive(level_filter.into())
        .parse_lossy(directives);
    let file_layer = layer
        .with_ansi_sanitization(false)
        .with_timer(UtcTime::new(Iso8601::DEFAULT))
        .with_writer(tracing_file)
        .with_filter(filter);
    layers.push(file_layer.boxed());

    try_init(layers)?;
    Ok(())
}

fn directives(config: &ConfigSalusAgent, level_filter: LevelFilter) -> String {
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

fn default_tracing_absolute_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    // `dirs2` exposes no dedicated log/state directory, so logs live under the
    // local data dir (Linux `~/.local/share`, macOS `~/Library/Application
    // Support`).
    let base = dirs2::data_local_dir().ok_or(Error::LogDir)?;
    Ok(log_file_in(&base, &defaults.app_name()))
}

/// Compose the default log file path: `<base>/<app>/<app>.log`.
fn log_file_in(base: &Path, app: &str) -> PathBuf {
    base.join(app).join(app).with_extension("log")
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use anyhow::Result;
    use config::{Config, FileFormat};
    use tracing::level_filters::LevelFilter;

    use super::{directives, log_file_in};
    use crate::config::ConfigSalusAgent;

    #[test]
    fn log_file_in_composes_app_dir_and_extension() {
        let path = log_file_in(Path::new("/base"), "salus-agent");
        assert_eq!(path, Path::new("/base/salus-agent/salus-agent.log"));
    }

    #[test]
    fn directives_map_each_level_filter() {
        let cfg = ConfigSalusAgent::default();
        assert_eq!(directives(&cfg, LevelFilter::TRACE), "trace");
        assert_eq!(directives(&cfg, LevelFilter::DEBUG), "debug");
        assert_eq!(directives(&cfg, LevelFilter::INFO), "info");
        assert_eq!(directives(&cfg, LevelFilter::WARN), "warn");
        assert_eq!(directives(&cfg, LevelFilter::ERROR), "error");
        // `OFF` has no level, so the base falls back to `info`.
        assert_eq!(directives(&cfg, LevelFilter::OFF), "info");
    }

    #[test]
    fn directives_appends_configured_directives() -> Result<()> {
        let cfg: ConfigSalusAgent = Config::builder()
            .add_source(config::File::from_str(
                "[tracing]\ndirectives = \"mycrate=debug\"\n",
                FileFormat::Toml,
            ))
            .build()?
            .try_deserialize()?;
        assert_eq!(directives(&cfg, LevelFilter::INFO), "info,mycrate=debug");
        Ok(())
    }
}
