// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

//! `cargo xtask dist <binary>`
//!
//! Generates shell completions (bash, zsh, fish) and a man page for the given
//! salus binary, copies the dual licenses, and — for `salusd`/`salus-agent` —
//! copies the systemd user unit and the example config. Each binary's output is
//! written to `dist/<binary>/`.
//!
//! # Usage
//!
//! ```text
//! cargo xtask dist salusd
//! cargo xtask dist salusc
//! cargo xtask dist salus-agent
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use clap::{Arg, ArgAction, Command};
use clap_complete::{Shell, generate_to};
use clap_mangen::Man;

fn main() -> Result<()> {
    let matches = Command::new("xtask")
        .subcommand_required(true)
        .subcommand(
            Command::new("dist")
                .about("Generate shell completions and man pages for a binary")
                .arg(
                    Arg::new("binary")
                        .required(true)
                        .help("Binary to generate artifacts for (salusd, salusc, salus-agent)"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("dist", sub)) => {
            let binary = sub.get_one::<String>("binary").expect("required");
            dist(binary)
        }
        _ => bail!("unknown subcommand"),
    }
}

fn dist(binary: &str) -> Result<()> {
    let mut cmd = match binary {
        "salusd" => salusd_command(),
        "salusc" => salusc_command(),
        "salus-agent" => salus_agent_command(),
        other => bail!("unknown binary '{other}'; expected one of: salusd, salusc, salus-agent"),
    };

    let out_dir = PathBuf::from("dist").join(binary);
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create output directory {}", out_dir.display()))?;

    generate_completions(binary, &mut cmd, &out_dir)?;
    generate_man_page(&cmd, &out_dir)?;
    copy_licenses(&out_dir)?;
    copy_systemd_units(binary, &out_dir)?;
    copy_example_config(binary, &out_dir)?;

    println!("Artifacts written to {}", out_dir.display());
    Ok(())
}

fn copy_licenses(out_dir: &Path) -> Result<()> {
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        fs::copy(name, out_dir.join(name))
            .with_context(|| format!("failed to copy {name} to {}", out_dir.display()))?;
    }
    Ok(())
}

fn copy_systemd_units(binary: &str, out_dir: &Path) -> Result<()> {
    let unit = match binary {
        "salusd" => "salusd.service",
        "salus-agent" => "salus-agent.service",
        _ => return Ok(()),
    };
    let src = PathBuf::from("packaging/systemd").join(unit);
    fs::copy(&src, out_dir.join(unit))
        .with_context(|| format!("failed to copy {}", src.display()))?;
    Ok(())
}

fn copy_example_config(binary: &str, out_dir: &Path) -> Result<()> {
    let example = match binary {
        "salusd" => "salusd.toml.example",
        "salus-agent" => "salus-agent.toml.example",
        _ => return Ok(()),
    };
    let src = PathBuf::from("packaging/arch/salus/examples").join(example);
    if src.exists() {
        fs::copy(&src, out_dir.join(example))
            .with_context(|| format!("failed to copy {}", src.display()))?;
    }
    Ok(())
}

// ── Completion generation ─────────────────────────────────────────────────────

fn generate_completions(binary: &str, cmd: &mut Command, out_dir: &Path) -> Result<()> {
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        generate_to(shell, cmd, binary, out_dir).with_context(|| {
            format!(
                "failed to generate {} completions for {binary}",
                shell_name(shell)
            )
        })?;
    }
    Ok(())
}

fn shell_name(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        _ => "unknown",
    }
}

// ── Man page generation ───────────────────────────────────────────────────────

fn generate_man_page(cmd: &Command, out_dir: &Path) -> Result<()> {
    let man = Man::new(cmd.clone());
    let file_name = format!("{}.1", cmd.get_name());
    let mut file = fs::File::create(out_dir.join(&file_name))
        .with_context(|| format!("failed to create man page file {file_name}"))?;
    man.render(&mut file)
        .with_context(|| format!("failed to render man page {file_name}"))?;
    Ok(())
}

// ── CLI command definitions ───────────────────────────────────────────────────
//
// These mirror the actual Cli structs in salusd/src/runtime/cli.rs and
// salusc/src/runtime/cli.rs without importing those crates. Keep these in sync
// with any CLI changes.

/// `salusd` — the secret store daemon
fn salusd_command() -> Command {
    Command::new("salusd")
        .version(env!("CARGO_PKG_VERSION"))
        .about("The daemon for the secret store")
        .arg(verbose_arg())
        .arg(quiet_arg())
        .arg(
            Arg::new("enable-std-output")
                .short('e')
                .long("enable-std-output")
                .action(ArgAction::SetTrue)
                .help("Enable logging to stdout/stderr"),
        )
        .arg(config_absolute_path_arg())
        .arg(
            Arg::new("tracing-absolute-path")
                .short('t')
                .long("tracing-absolute-path")
                .value_name("PATH")
                .help("Specify the absolute path to the tracing output file"),
        )
        .arg(
            Arg::new("database-absolute-path")
                .short('d')
                .long("database-absolute-path")
                .value_name("PATH")
                .help("Specify the absolute path to the database file"),
        )
        .arg(socket_path_arg())
}

/// `salusc` — the command line client for the daemon
fn salusc_command() -> Command {
    Command::new("salusc")
        .version(env!("CARGO_PKG_VERSION"))
        .about("The command line client for the salusd daemon")
        .subcommand_required(true)
        .arg(verbose_arg())
        .arg(quiet_arg())
        .arg(
            Arg::new("config-path")
                .short('c')
                .long("config-path")
                .value_name("PATH")
                .help("Specify a path to the config file"),
        )
        .arg(socket_path_arg())
        .arg(
            Arg::new("agent-socket-path")
                .short('a')
                .long("agent-socket-path")
                .value_name("PATH")
                .help("Specify the path to the salus-agent IPC socket"),
        )
        .subcommand(
            Command::new("shares")
                .about("Generate and print the secret shares (first-time init)")
                .arg(
                    Arg::new("num-shares")
                        .short('n')
                        .long("num-shares")
                        .value_name("N")
                        .default_value("5")
                        .help("The number of shares to create"),
                )
                .arg(
                    Arg::new("threshold")
                        .short('t')
                        .long("threshold")
                        .value_name("N")
                        .default_value("3")
                        .help("The number of shares required to reconstruct the secret"),
                ),
        )
        .subcommand(
            Command::new("unlock")
                .about("Reconstruct the key in the daemon from secret shares")
                .arg(
                    Arg::new("set")
                        .short('s')
                        .long("set")
                        .value_name("NAME")
                        .help("The named enrollment set to unlock with"),
                )
                .arg(
                    Arg::new("duration")
                        .short('f')
                        .long("for")
                        .value_name("SECONDS|forever")
                        .help("How long the daemon should hold the key (max 24h)"),
                ),
        )
        .subcommand(
            Command::new("lock")
                .about("Clear the daemon's unlocked key and any pending auto-clear timer"),
        )
        .subcommand(
            Command::new("store")
                .about("Store a value under a key")
                .arg(
                    Arg::new("key")
                        .short('k')
                        .long("key")
                        .value_name("KEY")
                        .required(true)
                        .help("The key to store the value under"),
                )
                .arg(
                    Arg::new("value")
                        .short('v')
                        .long("value")
                        .value_name("VALUE")
                        .required(true)
                        .help("The value to store"),
                ),
        )
        .subcommand(
            Command::new("read").about("Read a value by key").arg(
                Arg::new("key-opt")
                    .short('k')
                    .long("key-opt")
                    .value_name("KEY")
                    .help("The key to read the value from"),
            ),
        )
        .subcommand(
            Command::new("find")
                .about("Find keys matching a regex")
                .arg(
                    Arg::new("regex")
                        .value_name("REGEX")
                        .required(true)
                        .help("The regex to find keys with"),
                ),
        )
        .subcommand(
            Command::new("enroll")
                .about("Enroll a named set of shares for agent-assisted unlocking")
                .arg(
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .default_value("default")
                        .help("The name of the enrollment set"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Replace an existing set with the same name"),
                )
                .arg(
                    Arg::new("independent-auto")
                        .long("independent-auto")
                        .action(ArgAction::SetTrue)
                        .help("Store this set's automatic shares separately"),
                ),
        )
        .subcommand(
            Command::new("forget")
                .about("Remove a named enrolled set, or every set with --all")
                .arg(
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .conflicts_with("all")
                        .help("The name of the set to remove"),
                )
                .arg(
                    Arg::new("all")
                        .long("all")
                        .action(ArgAction::SetTrue)
                        .help("Remove every enrolled set"),
                ),
        )
        .subcommand(
            Command::new("enroll-status")
                .about("List the enrolled sets and whether the agent is reachable"),
        )
}

/// `salus-agent` — the login agent that holds enrolled shares
fn salus_agent_command() -> Command {
    Command::new("salus-agent")
        .version(env!("CARGO_PKG_VERSION"))
        .about("The login agent for the secret store")
        .arg(verbose_arg())
        .arg(quiet_arg())
        .arg(
            Arg::new("enable-std-output")
                .short('e')
                .long("enable-std-output")
                .action(ArgAction::SetTrue)
                .help("Enable logging to stdout/stderr"),
        )
        .arg(config_absolute_path_arg())
        .arg(
            Arg::new("tracing-absolute-path")
                .short('t')
                .long("tracing-absolute-path")
                .value_name("PATH")
                .help("Specify the absolute path to the tracing output file"),
        )
        .arg(
            Arg::new("socket-path")
                .short('s')
                .long("socket-path")
                .value_name("PATH")
                .help("Specify the path to the agent IPC socket"),
        )
}

// ── Shared argument helpers ───────────────────────────────────────────────────

fn verbose_arg() -> Arg {
    Arg::new("verbose")
        .short('v')
        .long("verbose")
        .action(ArgAction::Count)
        .help("Turn up logging verbosity (multiple will turn it up more)")
        .conflicts_with("quiet")
}

fn quiet_arg() -> Arg {
    Arg::new("quiet")
        .short('q')
        .long("quiet")
        .action(ArgAction::Count)
        .help("Turn down logging verbosity (multiple will turn it down more)")
        .conflicts_with("verbose")
}

fn config_absolute_path_arg() -> Arg {
    Arg::new("config-absolute-path")
        .short('c')
        .long("config-absolute-path")
        .value_name("PATH")
        .help("Specify the absolute path to the config file")
}

fn socket_path_arg() -> Arg {
    Arg::new("socket-path")
        .short('s')
        .long("socket-path")
        .value_name("PATH")
        .help("Specify the path to the IPC socket")
}
