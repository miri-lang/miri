// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use clap::{ArgAction, ArgGroup, Parser, Subcommand};
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

/// Agent configuration flavor for skill installation.
///
/// Specifies where skills are installed based on which agent tool uses them:
/// - `claude` → `.claude/skills/<name>/SKILL.md` (Claude Code reads this path)
/// - `agents`, `cursor`, `codex` → `.agents/skills/<name>/SKILL.md` (vendor-neutral path used by Cursor, Codex, OpenCode, Windsurf, Gemini CLI)
/// - `generic` → `skills/<name>/SKILL.md` (generic project-local path)
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentFlavor {
    /// Claude Code (`claude` tool)
    #[default]
    Claude,
    /// Vendor-neutral agents path (used by Cursor, Codex, OpenCode, Windsurf, Gemini CLI)
    Agents,
    /// Cursor (alias for `agents` path)
    Cursor,
    /// Codex (alias for `agents` path)
    Codex,
    /// Generic project-local path
    Generic,
}

/// Top-level CLI argument definition parsed by clap.
#[derive(Parser, Debug)]
#[command(
    name = "miri",
    version = version_ref(),
    about = "Miri Compiler",
    author = "Slavik Shynkarenko <slavik@slavikdev.com>",
    long_about = "Miri Compiler - a modern, GPU-first, statically-typed programming language.\n\n\
Global options:\n\n\
--verify-mir: Run the MIR verification pass after Perceus RC insertion, checking RC invariants \
(StorageLive/Dead balance, no RC ops on parameters). Disabled by default. Also enabled by \
setting MIRI_VERIFY_MIR to any non-empty value in the environment.\n\n\
--color: Control ANSI color codes in diagnostic output. `auto` (default) detects TTY and emits \
colors only if stderr is a terminal. `always` forces color codes on; useful for piping to \
another tool that supports them. `never` disables all color codes. Note: JSON format \
(`--format json`) never emits ANSI codes regardless of this setting."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(short, long, action = ArgAction::Count, global = true, help = "Increase verbosity level", help_heading = "Global options")]
    pub verbose: u8,

    #[arg(
        long,
        global = true,
        help = "Verify RC invariants after Perceus",
        help_heading = "Global options"
    )]
    pub verify_mir: bool,

    // The variants are named in the help text rather than enumerated by clap:
    // this flag is repeated on every subcommand, and a six-line value listing
    // there is documentation a tool pays for on every command it reads.
    #[arg(
        long,
        value_enum,
        default_value_t = ColorMode::Auto,
        global = true,
        help = "ANSI color in diagnostics: auto, always, never",
        help_heading = "Global options",
        hide_possible_values = true
    )]
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

    /// Explain a diagnostic code or list all codes in the registry
    #[command(group = ArgGroup::new("explain_mode").required(true).multiple(false))]
    Explain {
        /// Diagnostic code to explain (e.g. MER_TYP_010)
        #[arg(group = "explain_mode")]
        code: Option<String>,

        /// List all diagnostic codes in the registry
        #[arg(long, action = ArgAction::SetTrue, group = "explain_mode")]
        list: bool,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Run tests
    ///
    /// Discovers and executes all `@test` functions under the specified directory or file.
    /// Exit codes: 0 on success, 1 when a test fails and no files were rejected,
    /// 2 when any file was rejected from the test run (rejected files take priority,
    /// indicating tests never ran). The JSON envelope's exitCode matches the process status.
    Test {
        /// File or directory to test. Defaults to the current directory.
        /// Mutually exclusive with --dir.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,

        /// Filter tests by a substring in the path
        #[arg(long)]
        filter: Option<String>,

        /// Output format for test results
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,

        /// Directory to search for tests (mutually exclusive with positional PATH)
        #[arg(long, conflicts_with = "path")]
        dir: Option<PathBuf>,
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

    /// Manage embedded skills for AI agents
    #[command(subcommand)]
    Skill(SkillCommand),

    /// Verify that build artifacts are byte-reproducible
    #[command(subcommand)]
    Determinism(DeterminismCommand),

    /// Read part of a Miri source file: one function, or an outline of it
    #[command(group = ArgGroup::new("view_mode").required(true).multiple(false))]
    View {
        /// Path to the Miri source file
        #[arg(required = true)]
        path: PathBuf,

        /// Show one function: its name, or `Class.method` for a method
        #[arg(long = "fn", value_name = "NAME", group = "view_mode")]
        fn_name: Option<String>,

        /// List every declaration's signature, with no bodies
        #[arg(long, action = ArgAction::SetTrue, group = "view_mode")]
        outline: bool,

        /// With `--outline`, list only the public surface: no `runtime`
        /// bindings and no `private` or `protected` members
        #[arg(long, action = ArgAction::SetTrue, conflicts_with = "fn_name")]
        public: bool,

        /// Narrow `--fn` to the innermost block containing this text
        #[arg(long, value_name = "TEXT", requires = "fn_name")]
        around: Option<String>,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Rewrite a file to its canonical form
    Fmt {
        /// Path to the Miri source file to format
        #[arg(required = true)]
        path: PathBuf,

        /// Validate without writing; exit non-zero if file is not already canonical
        #[arg(long, action = ArgAction::SetTrue)]
        check: bool,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Apply source edits with re-validation
    Patch {
        /// Path to the Miri source file
        #[arg(required = true)]
        path: PathBuf,

        /// Function to patch: its name, or `Class.method` for a method (repeatable)
        #[arg(long = "replace-in-fn", value_name = "NAME")]
        fn_name: Vec<String>,

        /// Text to find in the function (canonical form); pairs positionally with --new
        #[arg(long, value_name = "TEXT")]
        old: Vec<String>,

        /// Text to replace it with; pairs positionally with --old
        #[arg(long, value_name = "TEXT")]
        new: Vec<String>,

        /// Read multi-line --old text from a file or stdin (-); pairs positionally with --new-file
        #[arg(long, value_name = "PATH|-")]
        old_file: Vec<String>,

        /// Read multi-line --new text from a file or stdin (-); pairs positionally with --old-file
        #[arg(long, value_name = "PATH|-")]
        new_file: Vec<String>,

        /// Function to replace wholly: its name, or `Class.method`; pairs with --body-file
        #[arg(long, value_name = "NAME")]
        replace_fn: Vec<String>,

        /// Read function body from a file or stdin (-); pairs positionally with --replace-fn
        #[arg(long, value_name = "PATH|-")]
        body_file: Vec<String>,

        /// Function to insert: its name, or `Class.method`; pairs with --body-file
        #[arg(long, value_name = "NAME")]
        insert_fn: Vec<String>,

        /// Declaration the new one follows; optional, pairs positionally with --insert-fn
        #[arg(long, value_name = "DECL")]
        after: Vec<String>,

        /// Guard against stale state: require this SHA-256 hash
        #[arg(long, value_name = "HEX")]
        expect_sha: Option<String>,

        /// Validate without writing
        #[arg(long, action = ArgAction::SetTrue)]
        check_only: bool,

        /// Print the diff without writing
        #[arg(long, action = ArgAction::SetTrue)]
        dry_run: bool,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillCommand {
    /// List all available skills
    List {
        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Show a skill's content
    Show {
        /// Name of the skill to display
        #[arg(required = true)]
        name: String,

        /// Format for a failure. The skill itself is always written as the
        /// markdown it is, so the output can be redirected into place; this
        /// chooses how a name that is not in the catalogue is reported.
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },

    /// Install one or more skills to an agent's configuration directory
    Install {
        /// Names of skills to install (all skills if empty)
        #[arg()]
        names: Vec<String>,

        /// Agent configuration flavor
        #[arg(long, value_enum, default_value_t = AgentFlavor::Claude)]
        agent: AgentFlavor,

        /// Target root directory for installation
        #[arg(long, default_value = ".")]
        target: PathBuf,

        /// Overwrite locally-modified files without prompting
        #[arg(long, action = clap::ArgAction::SetTrue)]
        force: bool,

        /// Output format (pretty or JSON)
        #[arg(long, value_enum, default_value_t = Format::Pretty)]
        format: Format,
    },
}

#[derive(Subcommand, Debug)]
pub enum DeterminismCommand {
    /// Check if an input builds deterministically
    Check {
        /// Path to the Miri source file to check
        #[arg(required = true)]
        path: PathBuf,

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
}
