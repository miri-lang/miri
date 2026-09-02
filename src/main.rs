// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use std::path::PathBuf;

use miri::cli::skill;
use miri::cli::{Cli, ColorMode, Commands, DeterminismCommand, Format, SkillCommand};
use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic};
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
            Commands::Check { path, format } => check_file(path, format, cli.verify_mir, cli.color),
            Commands::Dev { path, format } => dev_watch(path, format, cli.verify_mir, cli.color),
            Commands::Agent {} => serve_agent(),
            Commands::Explain { code, list, format } => explain_code(code, list, format, cli.color),
            Commands::View {
                path,
                fn_name,
                outline,
                public,
                around,
                format,
            } => view_file(path, fn_name, outline, public, around, format, cli.color),
            Commands::Fmt {
                path,
                check,
                format,
            } => fmt_file(path, check, format, cli.color),
            Commands::Patch {
                path,
                fn_name,
                old,
                new,
                old_file,
                new_file,
                replace_fn,
                body_file,
                insert_fn,
                after,
                expect_sha,
                check_only,
                dry_run,
                format,
            } => patch_file(
                path, fn_name, old, new, old_file, new_file, replace_fn, body_file, insert_fn,
                after, expect_sha, check_only, dry_run, format, cli.color,
            ),
            Commands::Fix {
                path,
                plan: _plan,
                apply,
                yes,
                allow_risky,
                format,
            } => fix_file(path, apply, yes, allow_risky, format, cli.color),
            Commands::Test {
                path,
                filter,
                format,
                dir,
            } => run_tests(
                path,
                filter,
                format,
                dir,
                cli.verbose,
                cli.verify_mir,
                cli.color,
            ),
            Commands::Determinism(cmd) => match cmd {
                DeterminismCommand::Check {
                    path,
                    release,
                    opt_level,
                    cpu_backend,
                    target,
                    format,
                } => check_determinism(
                    path,
                    BuildOptions {
                        out_path: None,
                        release,
                        opt_level,
                        cpu_backend,
                        target,
                        emit_native_host: true,
                    },
                    format,
                    cli.verify_mir,
                    cli.color,
                ),
            },
            Commands::Skill(cmd) => skill_command(cmd, cli.color),
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
    let Some(source) =
        miri::cli::source::read_or_report(&path, JsonCommand::Run, format, color_mode)
    else {
        std::process::exit(1);
    };

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
            Ok((exit_code, stdout_bytes, stderr_bytes, trap_code)) => {
                let exit_code = report_run(
                    exit_code,
                    &stdout_bytes,
                    &stderr_bytes,
                    trap_code,
                    start.elapsed().as_millis() as u64,
                );
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

/// Write the envelope for a program that compiled and ran, and say how it left.
///
/// `ok` reports the run rather than the status the program chose: a `main`
/// returning 7 succeeded at being compiled and run, and a caller that wants
/// that number reads `exitCode`. What makes it false is the program dying
/// rather than finishing — a runtime trap, or a death that produced no status
/// of its own at all.
fn report_run(
    exit_code: i32,
    stdout_bytes: &[u8],
    stderr_bytes: &[u8],
    trap_code: Option<String>,
    elapsed_ms: u64,
) -> i32 {
    let (stdout_tail, stdout_truncated) = tail_output(stdout_bytes, 8192);
    let (stderr_tail, stderr_truncated) = tail_output(stderr_bytes, 8192);

    // A trap report is only meaningful for a run that actually died. A program
    // that completed successfully cannot be carrying a trap, so its report is
    // ignored rather than allowed to turn a successful run into a failed one.
    let trap_code = trap_code.filter(|_| exit_code != 0);
    let has_trap = trap_code.is_some();
    let diagnostics = trap_code.map(trap_diagnostic).into_iter().collect();
    let completed = !has_trap && exit_code != NO_EXIT_STATUS;

    let envelope = DiagnosticsEnvelope::new(JsonCommand::Run, completed, diagnostics)
        .with_exit_code(exit_code)
        .with_duration_ms(elapsed_ms)
        .with_stdout(stdout_tail, stdout_truncated)
        .with_stderr(stderr_tail, stderr_truncated);

    println!("{}", miri::cli::serialize_envelope(&envelope));
    exit_code
}

/// The diagnostic a runtime trap is reported as.
///
/// The trap channel carries only the code, so the sentence a reader sees is
/// looked up from it here rather than travelling with it.
fn trap_diagnostic(code: String) -> JsonDiagnostic {
    let message = match code.as_str() {
        "MER_RT_001" => "division by zero",
        "MER_RT_002" => "remainder by zero",
        _ => "runtime trap",
    };
    JsonDiagnostic {
        severity: "error".to_string(),
        code: Some(code),
        message: message.to_string(),
        path: None,
        line: None,
        column: None,
        length: None,
        expected: None,
        actual: None,
        help: None,
        fix_safety: None,
        repair: None,
        related: vec![],
        preexisting: None,
    }
}

/// The status stood in for a program that finished without one of its own.
///
/// A process killed by a signal has no exit status to report, and the run
/// reports that stand-in rather than a number the program chose: a program
/// that exits with -1 is reported as 255, so the two cannot be confused.
const NO_EXIT_STATUS: i32 = -1;

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
    let Some(source) =
        miri::cli::source::read_or_report(&path, JsonCommand::Build, format, color_mode)
    else {
        std::process::exit(1);
    };

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

/// Check one file. The command's work and its rendering live in the CLI layer;
/// this arm only maps the outcome onto a process exit code.
fn check_file(
    path: PathBuf,
    format: Format,
    verify_mir: bool,
    color_mode: ColorMode,
) -> Result<()> {
    match miri::cli::check::run(&path, format, verify_mir, color_mode) {
        miri::cli::check::Outcome::Succeeded => Ok(()),
        miri::cli::check::Outcome::Failed => std::process::exit(1),
    }
}

/// Format one file to its canonical form.
fn fmt_file(path: PathBuf, check: bool, format: Format, color_mode: ColorMode) -> Result<()> {
    match miri::cli::fmt::run(&path, check, format, color_mode) {
        miri::cli::fmt::Outcome::Succeeded => Ok(()),
        miri::cli::fmt::Outcome::Failed => std::process::exit(1),
    }
}

/// Watch a file for changes and re-check on each change.
fn dev_watch(path: PathBuf, format: Format, verify_mir: bool, color_mode: ColorMode) -> Result<()> {
    match miri::cli::dev::run(path, format, verify_mir, color_mode) {
        miri::cli::dev::Outcome::Exited => Ok(()),
        miri::cli::dev::Outcome::Failed => std::process::exit(1),
    }
}

/// Serve JSON-RPC requests until the client closes stdin.
///
/// The session runs on this thread, which already has the stack the compiler's
/// recursive passes need, so every request is served with the same headroom a
/// one-shot command gets.
fn serve_agent() -> Result<()> {
    miri::cli::agent::run().context("the agent session ended in an I/O error")
}

/// Explain one diagnostic code. Rendering lives in the CLI layer; this arm only
/// maps the outcome onto a process exit code.
fn explain_code(
    code: Option<String>,
    list: bool,
    format: Format,
    color_mode: ColorMode,
) -> Result<()> {
    if list {
        miri::cli::explain::run_list(format);
        return Ok(());
    }
    match code {
        Some(code) => match miri::cli::explain::run(&code, format, color_mode) {
            miri::cli::explain::Outcome::Explained => Ok(()),
            miri::cli::explain::Outcome::UnknownCode => std::process::exit(1),
        },
        // The argument group rejects an invocation carrying neither a code nor
        // `--list`, so this stands as the exhaustive branch rather than a
        // reachable failure.
        None => std::process::exit(1),
    }
}

/// Run the fix command: emit repair suggestions or apply them.
fn fix_file(
    path: PathBuf,
    apply: bool,
    yes: bool,
    allow_risky: bool,
    format: Format,
    color_mode: ColorMode,
) -> Result<()> {
    match miri::cli::fix::run(&path, apply, yes, allow_risky, format, color_mode) {
        miri::cli::fix::Outcome::Succeeded => Ok(()),
        miri::cli::fix::Outcome::Refused | miri::cli::fix::Outcome::Failed => std::process::exit(1),
    }
}

/// View a scoped section of source code.
fn view_file(
    path: PathBuf,
    fn_name: Option<String>,
    outline: bool,
    public: bool,
    around: Option<String>,
    format: Format,
    color_mode: ColorMode,
) -> Result<()> {
    // Clap guarantees exactly one of `--fn` and `--outline` is present, so a
    // missing name here can only mean the outline was asked for.
    let shape = match fn_name {
        Some(name) => miri::cli::view::Shape::Function { name, around },
        None => {
            let _ = outline;
            miri::cli::view::Shape::Outline {
                public_only: public,
            }
        }
    };
    match miri::cli::view::run(&path, &shape, format, color_mode) {
        miri::cli::view::Outcome::Read => Ok(()),
        miri::cli::view::Outcome::Failed => std::process::exit(1),
    }
}

/// Apply source edits with re-validation.
#[allow(clippy::too_many_arguments)]
fn patch_file(
    path: PathBuf,
    fn_name: Vec<String>,
    old: Vec<String>,
    new: Vec<String>,
    old_file: Vec<String>,
    new_file: Vec<String>,
    replace_fn: Vec<String>,
    body_file: Vec<String>,
    insert_fn: Vec<String>,
    after: Vec<String>,
    expect_sha: Option<String>,
    check_only: bool,
    dry_run: bool,
    format: Format,
    color_mode: ColorMode,
) -> Result<()> {
    let request = miri::cli::patch::Request {
        functions: fn_name,
        old,
        new,
        old_file,
        new_file,
        replace_functions: replace_fn,
        body_file,
        insert_functions: insert_fn,
        after,
    };
    // A run that names no writable mode writes; --check-only and --dry-run each
    // hold the result back, and asking for both is asking for the stricter one.
    let mode = if check_only {
        miri::cli::patch::Mode::CheckOnly
    } else if dry_run {
        miri::cli::patch::Mode::DryRun
    } else {
        miri::cli::patch::Mode::Apply
    };

    let outcome = match miri::cli::patch::operations(&request) {
        Ok(operations) => miri::cli::patch::run(
            &path,
            &operations,
            expect_sha.as_deref(),
            mode,
            format,
            color_mode,
        ),
        Err(diagnostic) => {
            miri::cli::patch::report_malformed(&path, *diagnostic, format, color_mode)
        }
    };

    match outcome {
        miri::cli::patch::Outcome::Succeeded => Ok(()),
        miri::cli::patch::Outcome::Failed => std::process::exit(1),
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

/// Build DiagnosticsEnvelope for test results in JSON format.
fn build_test_envelope(
    summary: &miri::test_runner::TestSummary,
    elapsed_ms: u64,
    exit_code: i32,
) -> DiagnosticsEnvelope {
    let json_results = summary
        .results
        .iter()
        .map(|result| {
            let (code, line, column, expression, expected, actual, message) = result
                .failure
                .as_ref()
                .map(|f| {
                    (
                        Some(f.code.clone()),
                        f.line,
                        f.column,
                        f.expression.clone(),
                        f.expected.clone(),
                        f.actual.clone(),
                        f.message.clone(),
                    )
                })
                .unwrap_or((None, None, None, None, None, None, None));

            miri::diagnostics::json::JsonTestResult {
                path: result.path.clone(),
                name: result.name.clone(),
                outcome: outcome_to_string(result.outcome),
                detail: result.detail.clone(),
                code,
                line,
                column,
                expression,
                expected,
                actual,
                message,
            }
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

    DiagnosticsEnvelope::new(JsonCommand::Test, summary.is_green(), vec![])
        .with_duration_ms(elapsed_ms)
        .with_tests(json_summary)
        .with_exit_code(exit_code)
}

fn run_tests(
    path: Option<PathBuf>,
    filter: Option<String>,
    format: Format,
    dir: Option<PathBuf>,
    _verbose: u8,
    _verify_mir: bool,
    color_mode: ColorMode,
) -> Result<()> {
    let start = std::time::Instant::now();
    // A run names one file or one directory. `--dir` is the older spelling of
    // the directory form and clap keeps the two apart, so at most one is set.
    let target = path.or(dir).unwrap_or_else(|| PathBuf::from("."));

    // A path that is not there discovers no tests, and a run of no tests is
    // green. Reporting that would answer a mistyped path with a passing suite,
    // so the path is checked before anything is discovered.
    if !target.exists() {
        let diagnostic = miri::cli::source::missing(&target);
        miri::cli::source::report_unreadable(
            &target,
            &diagnostic,
            JsonCommand::Test,
            format,
            color_mode,
        );
        std::process::exit(1);
    }
    let summary = miri::test_runner::run_tests(&target, filter.as_deref())?;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    // Compute exit code once: rejected files take priority (incomplete run),
    // then test failures, then success.
    let exit_code = if !summary.rejected_files.is_empty() {
        2 // Any rejected file means tests never ran
    } else if summary.failed > 0 {
        1 // Tests failed
    } else {
        0 // All green
    };

    if format == Format::Json {
        let envelope = build_test_envelope(&summary, elapsed_ms, exit_code);
        let output = miri::cli::serialize_envelope(&envelope);
        println!("{}", output);
    } else {
        print!("{}", miri::test_runner::report::format_pretty(&summary));
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

/// Check determinism of a file: build twice and verify artifacts are identical.
fn check_determinism(
    path: PathBuf,
    build_options: BuildOptions,
    format: Format,
    verify_mir: bool,
    color_mode: ColorMode,
) -> Result<()> {
    match miri::cli::determinism::run(&path, format, verify_mir, color_mode, &build_options) {
        miri::cli::determinism::Outcome::DeterministicArtifacts => Ok(()),
        miri::cli::determinism::Outcome::NonDeterministicArtifacts => std::process::exit(1),
        miri::cli::determinism::Outcome::BuildFailed => std::process::exit(1),
    }
}

fn skill_command(cmd: SkillCommand, color_mode: ColorMode) -> Result<()> {
    let outcome = match cmd {
        SkillCommand::List { format } => skill::run_list(format, color_mode),
        SkillCommand::Show { name, format } => skill::run_show(&name, format, color_mode),
        SkillCommand::Install {
            names,
            agent,
            target,
            force,
            format,
        } => skill::run_install(&names, agent, &target, force, format, color_mode),
    };

    if outcome == skill::Outcome::Failed {
        std::process::exit(outcome.exit_code());
    }
    Ok(())
}
