// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

use miri::cli::{Cli, Commands, CpuBackend, TestFormat};
use miri::pipeline::{BuildOptions, Pipeline};

pub fn main() -> Result<()> {
    let cli = Cli::parse();

    // Propagate --verify-mir as an environment variable so the pipeline can
    // check it without requiring every call site to thread a flag through.
    if cli.verify_mir {
        // SAFETY: single-threaded at this point (before the pipeline starts).
        unsafe {
            std::env::set_var("MIRI_VERIFY_MIR", "1");
        }
    }

    match cli.command {
        Some(command) => match command {
            Commands::Run { path, program_args } => run_file(path, program_args, cli.verbose),
            Commands::Build {
                path,
                out,
                release,
                opt_level,
                cpu_backend,
            } => build_file(path, out, release, opt_level, cpu_backend, cli.verbose),
            Commands::Check { path } => check_file(path, cli.verbose),
            Commands::Test {
                filter,
                format,
                dir,
            } => run_tests(filter, format, dir, cli.verbose),
        },
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn run_file(path: PathBuf, _program_args: Vec<String>, _verbose: u8) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let pipeline = Pipeline::new();

    match pipeline.run(&source) {
        Ok(exit_code) => {
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e.report(&source));
            std::process::exit(1);
        }
    }
}

fn build_file(
    path: PathBuf,
    out: Option<PathBuf>,
    release: bool,
    opt_level: u8,
    cpu_backend: CpuBackend,
    _verbose: u8,
) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let pipeline = Pipeline::new();
    let build_options = BuildOptions {
        out_path: out,
        release,
        opt_level,
        cpu_backend,
    };

    match pipeline.build(&source, &build_options) {
        Ok(artifact_path) => {
            println!("Build successful. Artifact at: {}", artifact_path.display());
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e.report(&source));
            std::process::exit(1);
        }
    }
}

fn check_file(path: PathBuf, _verbose: u8) -> Result<()> {
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let pipeline = Pipeline::new();
    match pipeline.frontend(&source) {
        Ok(result) => {
            for warning in &result.type_checker.warnings {
                eprintln!(
                    "{}",
                    miri::error::format::format_diagnostic_full(&source, warning)
                );
            }
            let warning_count = result.type_checker.warnings.len();
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
            eprintln!("{}", e.report(&source));
            std::process::exit(1);
        }
    }
}

#[derive(Serialize)]
struct TestResult {
    path: String,
    status: String,
    error: Option<String>,
}

#[derive(Serialize)]
struct TestSummary {
    total: usize,
    passed: usize,
    failed: usize,
    results: Vec<TestResult>,
}

fn run_tests(filter: Option<String>, format: TestFormat, dir: PathBuf, _verbose: u8) -> Result<()> {
    let pipeline = Pipeline::new();
    let mut results = Vec::new();

    let test_files = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let is_miri_file = path.extension().is_some_and(|ext| ext == "mi");
            let in_tests_dir = path.to_string_lossy().contains("tests/");
            let has_test_in_name = path.to_string_lossy().contains("test");

            is_miri_file && (in_tests_dir || has_test_in_name)
        })
        .filter(|e| {
            filter
                .as_ref()
                .is_none_or(|f| e.path().to_string_lossy().contains(f))
        });

    for entry in test_files {
        let path = entry.path();
        let source_res = fs::read_to_string(path);

        let result = match source_res {
            Ok(source) => match pipeline.frontend(&source) {
                Ok(_) => TestResult {
                    path: path.to_string_lossy().into(),
                    status: "ok".to_string(),
                    error: None,
                },
                Err(e) => TestResult {
                    path: path.to_string_lossy().into(),
                    status: "fail".to_string(),
                    error: Some(e.report(&source)),
                },
            },
            Err(e) => TestResult {
                path: path.to_string_lossy().into(),
                status: "fail".to_string(),
                error: Some(format!("Failed to read file: {}", e)),
            },
        };
        results.push(result);
    }

    let total = results.len();
    let passed = results.iter().filter(|r| r.status == "ok").count();
    let failed = total - passed;

    let summary = TestSummary {
        total,
        passed,
        failed,
        results,
    };

    match format {
        TestFormat::Pretty => print_pretty_test_summary(&summary),
        TestFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
    }

    if failed > 0 {
        std::process::exit(101);
    }

    Ok(())
}

fn print_pretty_test_summary(summary: &TestSummary) {
    for result in &summary.results {
        println!("test {} ... {}", result.path, result.status);
        if let Some(err) = &result.error {
            eprintln!("---- error ----\n{}\n", err);
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed",
        if summary.failed > 0 { "failed" } else { "ok" },
        summary.passed,
        summary.failed
    );
}
