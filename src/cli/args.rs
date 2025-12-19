// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;

use crate::cli::version::version;

#[derive(Parser, Debug)]
#[command(name = "miri", version = version(), about = "Miri Compiler", author = "Slavik Shynkarenko <slavik@slavikdev.com>")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(short, long, action = ArgAction::Count, global = true, help = "Increase verbosity level")]
    pub verbose: u8,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a Miri source file
    Run {
        /// Path to the Miri source file to run
        #[arg(required = true)]
        path: PathBuf,

        /// Arguments to pass to the program
        #[arg(last = true)]
        program_args: Vec<String>,
    },

    /// Build a Miri source file
    Build {
        /// Path to the Miri source file to build
        #[arg(required = true)]
        path: PathBuf,

        /// Output path for the build artifact
        #[arg(short, long)]
        out: Option<PathBuf>,

        /// Build in release mode
        #[arg(long)]
        release: bool,

        /// Optimization level (0-3)
        #[arg(long, value_name = "LEVEL", default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
        opt_level: u8,
    },

    /// Run tests
    Test {
        /// Filter tests by a substring in the path
        #[arg(long)]
        filter: Option<String>,

        /// Output format for test results
        #[arg(long, value_enum, default_value_t = TestFormat::Pretty)]
        format: TestFormat,

        /// Directory to search for tests
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
}

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum TestFormat {
    #[default]
    Pretty,
    Json,
}
