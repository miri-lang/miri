// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;

use crate::cli::version::version_ref;

#[derive(Parser, Debug)]
#[command(name = "miri", version = version_ref(), about = "Miri Compiler", author = "Slavik Shynkarenko <slavik@slavikdev.com>")]
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

        /// Use interpreter instead of compilation (faster, for development)
        #[arg(long, short = 'i')]
        interpret: bool,

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

        /// CPU backend to use for code generation
        #[arg(long, value_enum, default_value_t = CpuBackend::Cranelift)]
        cpu_backend: CpuBackend,
    },

    /// Check a Miri source file for errors (type-check only, no code generation)
    Check {
        /// Path to the Miri source file to check
        #[arg(required = true)]
        path: PathBuf,
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

/// CPU backend for code generation.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuBackend {
    /// Cranelift: Fast compilation, good for development (default)
    #[default]
    Cranelift,
    /// LLVM: Optimized compilation (not yet implemented)
    Llvm,
}

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum TestFormat {
    #[default]
    Pretty,
    Json,
}
