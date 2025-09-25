// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use clap::error::ErrorKind;
use tracing::error;

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("There is no valid config directory")]
    ConfigDir,
    #[error("Unable to build a valid configuration")]
    ConfigBuild,
    #[error("Unable to deserialize config")]
    ConfigDeserialize,
    #[error("Unable to load a valid configuration")]
    ConfigLoad,
    #[error("Unable to initialize tracing")]
    TracingInit,
    #[error("Unable to initialize the database")]
    DatabaseInit,
    #[error("Unable to generate a nonce key")]
    NonceKeyGen,
    #[allow(dead_code)]
    #[error("Could not find CHECK_KEY in database")]
    CheckKeyNotFound,
    #[error("Error generating shares")]
    ShareGeneration,
    #[error("Store not unlocked")]
    StoreNotUnlocked,
    #[error("Invalid regex")]
    InvalidRegex,
    #[error("Unable to read next item from table iterator")]
    TableIterRead,
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn clap_or_error(err: anyhow::Error) -> i32 {
    let disp_err = || {
        eprintln!("{err:?}");
        1
    };
    match err.downcast_ref::<clap::Error>() {
        Some(e) => match e.kind() {
            ErrorKind::DisplayHelp => {
                eprintln!("{e}");
                0
            }
            ErrorKind::DisplayVersion => 0,
            ErrorKind::InvalidValue
            | ErrorKind::UnknownArgument
            | ErrorKind::InvalidSubcommand
            | ErrorKind::NoEquals
            | ErrorKind::ValueValidation
            | ErrorKind::TooManyValues
            | ErrorKind::TooFewValues
            | ErrorKind::WrongNumberOfValues
            | ErrorKind::ArgumentConflict
            | ErrorKind::MissingRequiredArgument
            | ErrorKind::MissingSubcommand
            | ErrorKind::InvalidUtf8
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::Io
            | ErrorKind::Format => disp_err(),
            _ => {
                eprintln!("Unknown ErrorKind");
                disp_err()
            }
        },
        None => disp_err(),
    }
}

pub(crate) fn success((): ()) -> i32 {
    0
}

#[cfg(test)]
mod test {
    use super::{clap_or_error, success};
    use anyhow::{Error, anyhow};
    use clap::{
        Command,
        error::ErrorKind::{self, DisplayHelp, DisplayVersion},
    };

    #[test]
    fn success_works() {
        assert_eq!(0, success(()));
    }

    #[test]
    fn clap_or_error_is_error() {
        assert_eq!(1, clap_or_error(anyhow!("test")));
    }

    #[test]
    fn clap_or_error_is_help() {
        let mut cmd = Command::new(env!("CARGO_PKG_NAME"));
        let error = cmd.error(DisplayHelp, "help");
        let clap_error = Error::new(error);
        assert_eq!(0, clap_or_error(clap_error));
    }

    #[test]
    fn clap_or_error_is_version() {
        let mut cmd = Command::new(env!("CARGO_PKG_NAME"));
        let error = cmd.error(DisplayVersion, "1.0");
        let clap_error = Error::new(error);
        assert_eq!(0, clap_or_error(clap_error));
    }

    #[test]
    fn clap_or_error_is_other_clap_error() {
        let mut cmd = Command::new(env!("CARGO_PKG_NAME"));
        let error = cmd.error(ErrorKind::InvalidValue, "Some failure case");
        let clap_error = Error::new(error);
        assert_eq!(1, clap_or_error(clap_error));
    }
}
