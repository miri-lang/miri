// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The corrected command `miri run` shows when program arguments arrive
//! without the `--` that separates them from the compiler's own.

use clap::builder::StyledStr;
use clap::error::{ContextKind, ContextValue, ErrorKind};

/// `error`, carrying the command line the caller meant when the argument clap
/// refused belongs to the program `miri run` starts.
///
/// `argv` is the command line as given, program name first. Only `run` takes
/// program arguments, and only a refused argument that is not itself a flag is
/// read as one: an unknown `--flag` is a different mistake, and the separator
/// would not be the fix for it.
pub fn with_separator_hint(mut error: clap::Error, argv: &[String]) -> clap::Error {
    if error.kind() != ErrorKind::UnknownArgument || argv.get(1).map(String::as_str) != Some("run")
    {
        return error;
    }
    let Some(ContextValue::String(unexpected)) = error.get(ContextKind::InvalidArg) else {
        return error;
    };
    if unexpected.starts_with('-') {
        return error;
    }
    let Some(position) = argv.iter().skip(2).position(|arg| arg == unexpected) else {
        return error;
    };
    let split = position + 2;

    let corrected: Vec<&str> = std::iter::once("miri")
        .chain(argv[1..split].iter().map(String::as_str))
        .chain(std::iter::once("--"))
        .chain(argv[split..].iter().map(String::as_str))
        .collect();
    error.insert(
        ContextKind::Suggested,
        ContextValue::StyledStrs(vec![StyledStr::from(format!(
            "arguments for the program go after `--`: {}",
            corrected.join(" ")
        ))]),
    );
    error
}
