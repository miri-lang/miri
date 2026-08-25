// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use std::fs;
use std::path::PathBuf;

use miri::cli::{Cli, ColorMode, Commands, Format};
use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};
use miri::error::diagnostic::to_json;
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
            Commands::Run {
                path,
                format,
                program_args,
            } => run_file(
                path,
                program_args,
                format,
                cli.verbose,
                cli.verify_mir,
                cli.color,
            ),
            Commands::Build {
                path,
                out,
                release,
                opt_level,
                cpu_backend,
                target,
                format,
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
                format,
                cli.verify_mir,
                cli.color,
            ),
            Commands::Check { path, format } => {
                check_file(path, format, cli.verbose, cli.verify_mir, cli.color)
            }
            Commands::Explain { code, format } => explain_code(&code, format, cli.color),
            Commands::Test {
                filter,
                format,
                dir,
            } => run_tests(filter, format, dir, cli.verbose, cli.verify_mir, cli.color),
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
    format: Format,
    _verbose: u8,
    verify_mir: bool,
    color_mode: ColorMode,
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

    let start = std::time::Instant::now();

    if format == Format::Json {
        // In JSON mode, capture output and emit envelope
        match pipeline.run_and_capture(&source, &program_args) {
            Ok((exit_code, stdout_bytes, stderr_bytes)) => {
                let elapsed_ms = start.elapsed().as_millis() as u64;

                let (stdout_tail, stdout_truncated) = tail_output(&stdout_bytes, 8192);
                let (stderr_tail, stderr_truncated) = tail_output(&stderr_bytes, 8192);

                let envelope = DiagnosticsEnvelope::new(JsonCommand::Run, exit_code == 0, vec![])
                    .with_exit_code(exit_code)
                    .with_duration_ms(elapsed_ms)
                    .with_stdout(stdout_tail, stdout_truncated)
                    .with_stderr(stderr_tail, stderr_truncated);

                let output = miri::cli::serialize_envelope(&envelope);
                println!("{}", output);

                if exit_code != 0 {
                    std::process::exit(exit_code);
                }
                Ok(())
            }
            Err(e) => {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                let diags = e.to_diagnostics();
                let json_diags = diags
                    .iter()
                    .map(|d| to_json(d, &source, pipeline.source_path()))
                    .collect();
                let envelope = DiagnosticsEnvelope::new(JsonCommand::Run, false, json_diags)
                    .with_exit_code(1)
                    .with_duration_ms(elapsed_ms);

                let output = miri::cli::serialize_envelope(&envelope);
                println!("{}", output);
                std::process::exit(1);
            }
        }
    } else {
        // In pretty mode, use the existing run() method which prints output
        match pipeline.run(&source, &program_args) {
            Ok(exit_code) => {
                if exit_code != 0 {
                    std::process::exit(exit_code);
                }
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    e.report_with_path_and_color(
                        &source,
                        pipeline.source_path(),
                        color_mode.into()
                    )
                );
                std::process::exit(1);
            }
        }
    }
}

/// Extract the last N bytes of a UTF-8 string, truncating on char boundaries.
/// Returns (tail_string, was_truncated).
fn tail_output(bytes: &[u8], max_len: usize) -> (String, bool) {
    if bytes.len() <= max_len {
        (String::from_utf8_lossy(bytes).into_owned(), false)
    } else {
        // Start from the end and walk backwards to find a char boundary
        let start = bytes.len() - max_len;
        // Align to the next valid char boundary
        let mut aligned_start = start;
        while aligned_start < bytes.len() && !is_char_boundary(bytes, aligned_start) {
            aligned_start += 1;
        }
        let tail = &bytes[aligned_start..];
        (String::from_utf8_lossy(tail).into_owned(), true)
    }
}

/// Check if a byte position is a UTF-8 character boundary.
fn is_char_boundary(bytes: &[u8], pos: usize) -> bool {
    if pos > bytes.len() {
        return false;
    }
    if pos == 0 || pos == bytes.len() {
        return true;
    }
    // UTF-8 continuation bytes have the form 10xxxxxx
    // A char boundary is where a byte does NOT have the 10xxxxxx pattern
    (bytes[pos] & 0xc0) != 0x80
}

fn build_file(
    path: PathBuf,
    build_options: BuildOptions,
    format: Format,
    verify_mir: bool,
    color_mode: ColorMode,
) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    if let Some(dir) = abs_path.parent() {
        pipeline = pipeline.with_source_dir(dir.to_path_buf());
    }
    pipeline = pipeline.with_source_path(abs_path.display().to_string());

    let start = std::time::Instant::now();
    match pipeline.build(&source, &build_options) {
        Ok(artifact_path) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if format == Format::Json {
                let envelope = DiagnosticsEnvelope::new(JsonCommand::Build, true, vec![])
                    .with_artifact(artifact_path.display().to_string())
                    .with_exit_code(0)
                    .with_duration_ms(elapsed_ms);
                let output = miri::cli::serialize_envelope(&envelope);
                println!("{}", output);
            } else {
                println!("Build successful. Artifact at: {}", artifact_path.display());
            }
            Ok(())
        }
        Err(e) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if format == Format::Json {
                let diags = e.to_diagnostics();
                let json_diags = diags
                    .iter()
                    .map(|d| to_json(d, &source, pipeline.source_path()))
                    .collect();
                let envelope = DiagnosticsEnvelope::new(JsonCommand::Build, false, json_diags)
                    .with_exit_code(1)
                    .with_duration_ms(elapsed_ms);
                let output = miri::cli::serialize_envelope(&envelope);
                println!("{}", output);
            } else {
                eprintln!(
                    "{}",
                    e.report_with_path_and_color(
                        &source,
                        pipeline.source_path(),
                        color_mode.into()
                    )
                );
            }
            std::process::exit(1);
        }
    }
}

fn check_file(
    path: PathBuf,
    format: Format,
    _verbose: u8,
    verify_mir: bool,
    color_mode: ColorMode,
) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    if let Some(dir) = abs_path.parent() {
        pipeline = pipeline.with_source_dir(dir.to_path_buf());
    }
    pipeline = pipeline.with_source_path(abs_path.display().to_string());

    let start = std::time::Instant::now();
    match pipeline.frontend(&source) {
        Ok(result) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if format == Format::Json {
                let warnings = result.type_checker.warnings();
                let json_diags = warnings
                    .iter()
                    .map(|w| to_json(w, &source, pipeline.source_path()))
                    .collect();
                // ok = true when the frontend succeeded (no errors), regardless of warnings.
                // Warnings do not fail a check; only errors do.
                let envelope = DiagnosticsEnvelope::new(JsonCommand::Check, true, json_diags)
                    .with_exit_code(0)
                    .with_duration_ms(elapsed_ms);
                let output = miri::cli::serialize_envelope(&envelope);
                println!("{}", output);
            } else {
                for warning in result.type_checker.warnings() {
                    eprintln!(
                        "{}",
                        miri::error::format::format_diagnostic_with_color(
                            &source,
                            warning,
                            pipeline.source_path(),
                            color_mode.into(),
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
            }
            Ok(())
        }
        Err(e) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if format == Format::Json {
                let diags = e.to_diagnostics();
                let json_diags = diags
                    .iter()
                    .map(|d| to_json(d, &source, pipeline.source_path()))
                    .collect();
                let envelope = DiagnosticsEnvelope::new(JsonCommand::Check, false, json_diags)
                    .with_exit_code(1)
                    .with_duration_ms(elapsed_ms);
                let output = miri::cli::serialize_envelope(&envelope);
                println!("{}", output);
            } else {
                eprintln!(
                    "{}",
                    e.report_with_path_and_color(
                        &source,
                        pipeline.source_path(),
                        color_mode.into()
                    )
                );
            }
            std::process::exit(1);
        }
    }
}

/// Explain one diagnostic code. Rendering lives in the CLI layer; this arm only
/// maps the outcome onto a process exit code.
fn explain_code(code: &str, format: Format, color_mode: ColorMode) -> Result<()> {
    match miri::cli::explain::run(code, format, color_mode) {
        miri::cli::explain::Outcome::Explained => Ok(()),
        miri::cli::explain::Outcome::UnknownCode => std::process::exit(1),
    }
}

fn outcome_to_string(outcome: miri::test_runner::Outcome) -> String {
    match outcome {
        miri::test_runner::Outcome::Passed => "passed".to_string(),
        miri::test_runner::Outcome::Failed => "failed".to_string(),
        miri::test_runner::Outcome::Ignored => "ignored".to_string(),
        miri::test_runner::Outcome::ExpectedFailure => "expected_failure".to_string(),
        miri::test_runner::Outcome::UnexpectedPass => "unexpected_pass".to_string(),
        miri::test_runner::Outcome::Crashed => "crashed".to_string(),
        miri::test_runner::Outcome::RunnerFault => "runner_fault".to_string(),
    }
}

fn rejection_reason_to_string(reason: miri::test_runner::RejectionReason) -> String {
    match reason {
        miri::test_runner::RejectionReason::Unparseable => "unparseable".to_string(),
        miri::test_runner::RejectionReason::DeclaresMain => "declares_main".to_string(),
        miri::test_runner::RejectionReason::TopLevelStatements => {
            "top_level_statements".to_string()
        }
    }
}

fn run_tests(
    filter: Option<String>,
    format: Format,
    dir: PathBuf,
    _verbose: u8,
    _verify_mir: bool,
    _color_mode: ColorMode,
) -> Result<()> {
    let start = std::time::Instant::now();
    let summary = miri::test_runner::run_tests(&dir, filter.as_deref())?;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    if format == Format::Json {
        // Wrap test summary in DiagnosticsEnvelope
        let json_results = summary
            .results
            .iter()
            .map(|result| miri::diagnostics::json::JsonTestResult {
                path: result.path.clone(),
                name: result.name.clone(),
                outcome: outcome_to_string(result.outcome),
                detail: result.detail.clone(),
            })
            .collect();

        let json_rejected = summary
            .rejected_files
            .iter()
            .map(|rf| miri::diagnostics::json::JsonRejectedFile {
                path: rf.path.clone(),
                reason: rejection_reason_to_string(rf.reason),
            })
            .collect();

        let json_summary = miri::diagnostics::json::JsonTestSummary {
            total: summary.total,
            passed: summary.passed,
            failed: summary.failed,
            ignored: summary.ignored,
            results: json_results,
            rejected_files: json_rejected,
        };

        let envelope = DiagnosticsEnvelope::new(JsonCommand::Test, summary.is_green(), vec![])
            .with_duration_ms(elapsed_ms)
            .with_tests(json_summary)
            .with_exit_code(if summary.is_green() { 0 } else { 101 });

        let output = miri::cli::serialize_envelope(&envelope);
        println!("{}", output);
    } else {
        print!("{}", miri::test_runner::report::format_pretty(&summary));
    }

    if !summary.is_green() {
        std::process::exit(101);
    }

    Ok(())
}
