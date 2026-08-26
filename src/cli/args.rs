// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;

use crate::cli::version::version_ref;
pub use crate::codegen::{BuildTarget, CpuBackend};

/// Output format for commands (pretty-printed or JSON).
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// Pretty-printed output (default).
    #[default]
    Pretty,
    /// JSON-formatted output.
    Json,
}

/// Color output mode.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Detect color support based on whether stderr is a TTY (default).
    #[default]
    Auto,
    /// Always emit ANSI color codes.
    Always,
    /// Never emit ANSI color codes.
    Never,
}

impl From<ColorMode> for crate::error::format::ColorChoice {
    fn from(mode: ColorMode) -> Self {
        match mode {
            ColorMode::Auto => crate::error::format::ColorChoice::Auto,
            ColorMode::Always => crate::error::format::ColorChoice::Always,
            ColorMode::Never => crate::error::format::ColorChoice::Never,
        }
    }
}

/// Top-level CLI argument definition parsed by clap.
#[derive(Parser, Debug)]
#[command(name = "miri", version = version_ref(), about = "Miri Compiler", author = "Slavik Shynkarenko <slavik@slavikdev.com>")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(short, long, action = ArgAction::Count, global = true, help = "Increase verbosity level")]
    pub verbose: u8,

    /// Run the MIR verification pass after Perceus RC insertion.
    ///
    /// When enabled, the compiler checks RC invariants (StorageLive/Dead balance,
    /// no RC ops on parameters) and reports any violations as errors. Disabled
    /// by default. Also enabled by setting MIRI_VERIFY_MIR to any non-empty value in the environment.
    #[arg(long, global = true)]
    pub verify_mir: bool,

    /// Control ANSI color codes in diagnostic output.
    ///
    /// `auto` (default) detects TTY and emits colors only if stderr is a terminal.
    /// `always` forces color codes on; useful for piping to another tool that supports them.
    /// `never` disables all color codes.
    /// Note: JSON format (`--format json`) never emits ANSI codes regardless of this setting.
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true)]
    pub color: ColorMode,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a Miri source file
    Run {
        /// Path to the Miri source file to run
        #[arg(required = true)]
        path: PathBuf,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,

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

        /// Build target. `native` (default) emits an executable for the host
        /// platform. `web-gpu` emits a browser-runnable HTML bundle that
        /// dispatches `gpu fn` kernels through WebGPU/WGSL.
        #[arg(long, value_enum, default_value_t = BuildTarget::Native)]
        target: BuildTarget,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Check a Miri source file for errors (type-check only, no code generation)
    Check {
        /// Path to the Miri source file to check
        #[arg(required = true)]
        path: PathBuf,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Watch a file and re-check on changes
    Dev {
        /// Path to the Miri source file to watch
        #[arg(required = true)]
        path: PathBuf,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Explain a diagnostic code
    Explain {
        /// Diagnostic code to explain (e.g. MER_TYP_010)
        #[arg(required = true)]
        code: String,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Run tests
    Test {
        /// Filter tests by a substring in the path
        #[arg(long)]
        filter: Option<String>,

        /// Output format for test results
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,

        /// Directory to search for tests
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },

    /// Serve JSON-RPC requests over stdin and stdout
    ///
    /// One compiler process answers many requests, so a tool driving the
    /// compiler pays the start-up cost once instead of once per invocation.
    Agent {},

    /// Emit repair suggestions for compiler diagnostics
    Fix {
        /// Path to the Miri source file to fix
        #[arg(required = true)]
        path: PathBuf,

        /// Report the repairs without modifying any file. This is the default.
        #[arg(long, action = ArgAction::SetTrue, conflicts_with = "apply")]
        plan: bool,

        /// Apply repairs to the source file
        #[arg(long, action = ArgAction::SetTrue)]
        apply: bool,

        /// Confirm applying the repairs. Required by `--apply` when there is no
        /// terminal to confirm at.
        #[arg(long, action = ArgAction::SetTrue)]
        yes: bool,

        /// Allow applying repairs classified as risky (api-changing, target-changing,
        /// or requires-human-review). Without this flag, `--apply` refuses to apply
        /// such repairs.
        #[arg(long, action = ArgAction::SetTrue)]
        allow_risky: bool,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },
}
