// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use std::fs;
use std::path::PathBuf;

use miri::cli::{Cli, Commands, TestFormat};
use miri::pipeline::{BuildOptions, Pipeline};

pub fn main() -> Result<()> {
    let cli = Cli::parse();

    // Run the whole command on a worker thread with a large stack. Compilation
    // recurses on expression-nesting depth (type inference and MIR lowering both
    // walk the expression tree), so a deep-but-valid input — e.g. a long
    // left-associative `a + b + c + ...` chain the parser builds into a deep
    // left-leaning AST — would otherwise overflow the fixed main-thread stack
    // and abort the process. A generous stack raises that ceiling far beyond any
    // realistic program while keeping the recursive passes simple.
    const COMPILE_STACK_SIZE: usize = 512 * 1024 * 1024;
    let worker = std::thread::Builder::new()
        .stack_size(COMPILE_STACK_SIZE)
        .spawn(move || run_command(cli))
        .context("failed to spawn compiler worker thread")?;
    match worker.join() {
        Ok(result) => result,
        // Propagate a panic from the worker as if it happened on this thread.
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn run_command(cli: Cli) -> Result<()> {
    match cli.command {
        Some(command) => match command {
            Commands::Run { path, program_args } => {
                run_file(path, program_args, cli.verbose, cli.verify_mir)
            }
            Commands::Build {
                path,
                out,
                release,
                opt_level,
                cpu_backend,
                target,
            } => build_file(
                path,
                BuildOptions {
                    out_path: out,
                    release,
                    opt_level,
                    cpu_backend,
                    target,
                    // The CLI emits the native host binary alongside a web-gpu
                    // bundle; the flag is a no-op for the native target.
                    emit_native_host: true,
                },
                cli.verify_mir,
            ),
            Commands::Check { path } => check_file(path, cli.verbose, cli.verify_mir),
            Commands::Test {
                filter,
                format,
                dir,
            } => run_tests(filter, format, dir, cli.verbose, cli.verify_mir),
        },
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn run_file(
    path: PathBuf,
    program_args: Vec<String>,
    _verbose: u8,
    verify_mir: bool,
) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    // Canonicalize first so that a bare filename like "main.mi" resolves to an
    // absolute path whose parent is the working directory, not an empty path.
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    if let Some(dir) = abs_path.parent() {
        pipeline = pipeline.with_source_dir(dir.to_path_buf());
    }
    pipeline = pipeline.with_source_path(abs_path.display().to_string());

    match pipeline.run(&source, &program_args) {
        Ok(exit_code) => {
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e.report_with_path(&source, pipeline.source_path()));
            std::process::exit(1);
        }
    }
}

fn build_file(path: PathBuf, build_options: BuildOptions, verify_mir: bool) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    if let Some(dir) = abs_path.parent() {
        pipeline = pipeline.with_source_dir(dir.to_path_buf());
    }
    pipeline = pipeline.with_source_path(abs_path.display().to_string());

    match pipeline.build(&source, &build_options) {
        Ok(artifact_path) => {
            println!("Build successful. Artifact at: {}", artifact_path.display());
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e.report_with_path(&source, pipeline.source_path()));
            std::process::exit(1);
        }
    }
}

fn check_file(path: PathBuf, _verbose: u8, verify_mir: bool) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    if let Some(dir) = abs_path.parent() {
        pipeline = pipeline.with_source_dir(dir.to_path_buf());
    }
    pipeline = pipeline.with_source_path(abs_path.display().to_string());
    match pipeline.frontend(&source) {
        Ok(result) => {
            for warning in result.type_checker.warnings() {
                eprintln!(
                    "{}",
                    miri::error::format::format_diagnostic(
                        &source,
                        warning,
                        pipeline.source_path(),
                    )
                );
            }
            let warning_count = result.type_checker.warnings().len();
            if warning_count > 0 {
                println!(
                    "Check passed. No errors found. {} warning(s) emitted.",
                    warning_count
                );
            } else {
                println!("Check passed. No errors or warnings found.");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e.report_with_path(&source, pipeline.source_path()));
            std::process::exit(1);
        }
    }
}

fn run_tests(
    filter: Option<String>,
    format: TestFormat,
    dir: PathBuf,
    _verbose: u8,
    _verify_mir: bool,
) -> Result<()> {
    let test_format = match format {
        TestFormat::Pretty => miri::test_runner::TestFormat::Pretty,
        TestFormat::Json => miri::test_runner::TestFormat::Json,
    };

    let summary = miri::test_runner::run_tests(&dir, filter.as_deref(), test_format)?;

    if !summary.is_green() {
        std::process::exit(101);
    }

    Ok(())
}
