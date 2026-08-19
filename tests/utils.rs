// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use assert_cmd::{pkg_name, Command};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tempfile::NamedTempFile;

/// Serializes execution of GPU programs across concurrently-running tests.
///
/// Each executed GPU program spawns its own process that creates, uses, and
/// tears down a real GPU device. When many such processes run at once (the
/// integration suite is multi-threaded), the platform GPU driver intermittently
/// crashes one child with `SIGSEGV`/`SIGTRAP` during device teardown — the
/// program prints the correct result, then dies while releasing the device.
/// Holding this lock around a GPU program's run ensures only one device is being
/// created/torn down at a time, which removes the driver contention. Only the
/// execution step is serialized; compilation of every other test stays parallel.
static GPU_RUN_SERIAL: Mutex<()> = Mutex::new(());

/// True when `input` compiles to a program that drives the GPU at runtime, and
/// the GPU value suite is active. Such runs must be serialized (see
/// [`GPU_RUN_SERIAL`]); all other work stays parallel.
fn needs_gpu_serialization(command: &str, input: &str) -> bool {
    cfg!(feature = "gpu_hardware") && command == "run" && input.contains("system.gpu")
}

pub const BINARY_NAME: &str = pkg_name!();
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn miri_cmd() -> Command {
    Command::new(assert_cmd::cargo_bin!("miri"))
}

pub struct CompilerResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CompilerResult {
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// Execute Miri binary with given command, forwarding `program_args` to the
/// compiled program after a `--` separator when any are given.
fn exec_miri(command: &str, input: &str, program_args: &[&str]) -> CompilerResult {
    use std::path::PathBuf;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", input).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    // Set MIRI_STDLIB_PATH so the compiler can find prelude and stdlib modules
    // from single-file tests running in temporary directories.
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let gpu_run = needs_gpu_serialization(command, input);
    // A GPU program that crashes only intermittently is retried a few times: the
    // platform driver occasionally kills a child with a signal while it tears the
    // device down, *after* it has already produced the correct result. `miri run`
    // surfaces a signal-killed child as exit code 255 (its `unwrap_or(-1)` for a
    // `None` exit code), so that specific code — and only for a GPU run — marks a
    // transient teardown crash worth retrying. A genuine error exits with its own
    // code and is returned immediately.
    let max_attempts = if gpu_run { 3 } else { 1 };
    const SIGNAL_KILLED_CHILD_CODE: i32 = 255;

    let mut result = None;
    for attempt in 1..=max_attempts {
        let mut cmd = miri_cmd();
        cmd.env("RUST_BACKTRACE", "1")
            .env("MIRI_LEAK_CHECK", "1")
            // Findings are fatal: every program the suite compiles is verifier
            // corpus, so an RC seam introduced anywhere fails the test that
            // compiles it rather than printing a warning nobody reads.
            .env("MIRI_VERIFY_MIR", "1")
            .env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
            // Prevent linker-override env vars from leaking in from concurrent tests.
            .env_remove("MIRI_CC")
            .env_remove("CC")
            .arg(command)
            .arg(&path);

        if !program_args.is_empty() {
            cmd.arg("--");
            cmd.args(program_args);
        }

        // Serialize GPU program runs so only one process creates/tears down a
        // device at a time; a poisoned lock is irrelevant (it guards no state).
        let output = {
            let _gpu_guard =
                gpu_run.then(|| GPU_RUN_SERIAL.lock().unwrap_or_else(|e| e.into_inner()));
            cmd.output().unwrap()
        };

        let transient_crash = gpu_run && output.status.code() == Some(SIGNAL_KILLED_CHILD_CODE);
        result = Some(CompilerResult {
            success: output.status.success(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        });
        if !transient_crash || attempt == max_attempts {
            break;
        }
    }
    result.expect("at least one attempt runs")
}

/// Run Miri binary with 'check' command (type-checking only)
pub fn miri_check(input: &str) -> CompilerResult {
    exec_miri("check", input, &[])
}

/// Run Miri binary with 'build' command (compilation)
pub fn miri_build(input: &str) -> CompilerResult {
    exec_miri("build", input, &[])
}

/// Run Miri binary with 'run' command (compilation + execution)
pub fn miri_run(input: &str) -> CompilerResult {
    exec_miri("run", input, &[])
}

/// Run Miri binary with 'run' command, forwarding `args` to the compiled program.
pub fn miri_run_with_args(input: &str, args: &[&str]) -> CompilerResult {
    exec_miri("run", input, args)
}

/// Run Miri binary with 'run' command with an additional environment variable.
/// Sets both the standard variables and the provided extra variable.
pub fn miri_run_with_env(input: &str, env_var: &str, env_value: &str) -> CompilerResult {
    use std::path::PathBuf;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", input).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let mut cmd = miri_cmd();
    cmd.env("RUST_BACKTRACE", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        .env(env_var, env_value)
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .arg("run")
        .arg(&path);

    let output = cmd.output().unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Run a multi-file Miri project.
///
/// `files` is a slice of `(relative_path, content)` pairs. The first file is
/// used as the entry point (`miri run <first_path>`). All files are written
/// into a temporary directory and the compiler is invoked with that directory
/// as the working directory. `MIRI_STDLIB_PATH` is set to the project's own
/// stdlib so it remains accessible even when CWD changes.
pub fn miri_run_project(files: &[(&str, &str)]) -> CompilerResult {
    use std::fs;
    use tempfile::tempdir;

    assert!(
        !files.is_empty(),
        "miri_run_project: files list must not be empty"
    );

    let temp_dir = tempdir().unwrap();

    for (rel_path, content) in files {
        let file_path = temp_dir.path().join(rel_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(file_path, content).unwrap();
    }

    let entry_file = files[0].0;
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let mut cmd = miri_cmd();
    let output = cmd
        .env("RUST_BACKTRACE", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        // Prevent linker-override env vars from leaking in from concurrent tests.
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .current_dir(temp_dir.path())
        .arg("run")
        .arg(entry_file)
        .output()
        .unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Strip ANSI escape codes from a string
pub fn strip_ansi(s: &str) -> String {
    static ANSI_RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = ANSI_RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap());
    re.replace_all(s, "").to_string()
}

/// Check that the output contains the expected error messages
pub fn check_error_output(source: &str, expected_parts: &[&str]) {
    let result = miri_check(source);
    let output = result.output();
    let clean_output = strip_ansi(&output);

    for part in expected_parts {
        assert!(
            clean_output.contains(part),
            "Output did not contain expected part.\nExpected: '{}'\nActual Output:\n{}",
            part,
            clean_output
        );
    }
}
