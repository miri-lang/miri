// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

const SIMPLE_MAIN: &str = r#"fn main() int
    42
"#;

#[test]
fn test_build_valid_file() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .assert()
        .success()
        .stdout(predicates::str::contains("Build successful"));
}

#[test]
fn test_build_with_output() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let out_dir = tempfile::tempdir().unwrap();
    let out_path_buf = out_dir.path().join("out_exe");
    let out_path = out_path_buf.to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--out")
        .arg(out_path)
        .assert()
        .success();
}

#[test]
fn test_build_release() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--release")
        .assert()
        .success();
}

#[test]
fn test_build_opt_level() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--opt-level")
        .arg("3")
        .assert()
        .success();
}

#[test]
fn test_build_invalid_opt_level() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--opt-level")
        .arg("4")
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid value '4'"));
}

#[test]
fn test_build_cpu_backend_cranelift() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--cpu-backend")
        .arg("cranelift")
        .assert()
        .success();
}

#[test]
fn test_build_cpu_backend_llvm() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--cpu-backend")
        .arg("llvm")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "LLVM backend is not yet available",
        ));
}

// ============================================================
// Executable output tests: verify the binary runs and prints
// ============================================================

/// The canonical hello-world program in Miri.
///
/// Uses `use system.io` and a `println` call so the runtime library
/// must be discovered and linked automatically by the build pipeline.
const HELLO_WORLD: &str = r#"use system.io

fn main()
    let msg = "Hello, Miri!"
    println(msg)
"#;

/// Build a script-mode hello-world (no explicit `main`) and run it.
///
/// The pipeline must auto-wrap the script in `main`, link `miri_rt`,
/// and produce a runnable executable without any manual linker flags.
const HELLO_WORLD_SCRIPT: &str = r#"use system.io

println("Hello from script!")
"#;

/// `miri build --out <path>` must produce a binary that, when executed
/// directly (no miri involved), prints the expected output and exits 0.
#[test]
fn test_build_produces_runnable_executable_with_output() {
    let source_file = create_test_file(HELLO_WORLD);
    let out_dir = tempfile::tempdir().unwrap();
    let out_path_buf = out_dir.path().join("out_exe");
    let out_path = out_path_buf.as_path();

    // Step 1: build succeeds.
    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source_file.path())
        .arg("--out")
        .arg(out_path)
        .assert()
        .success();

    // Step 2: execute the binary directly (no miri, no env overrides).
    let output = Command::new(out_path)
        .output()
        .expect("built binary should be executable");

    assert!(
        output.status.success(),
        "binary exited with non-zero code; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Hello, Miri!"),
        "expected 'Hello, Miri!' in stdout, got: {:?}",
        stdout
    );
}

/// Script-mode source (no `main`) must also build and produce output.
#[test]
fn test_build_script_mode_produces_runnable_executable() {
    let source_file = create_test_file(HELLO_WORLD_SCRIPT);
    let out_dir = tempfile::tempdir().unwrap();
    let out_path_buf = out_dir.path().join("out_exe");
    let out_path = out_path_buf.as_path();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source_file.path())
        .arg("--out")
        .arg(out_path)
        .assert()
        .success();

    let output = Command::new(out_path)
        .output()
        .expect("built binary should be executable");

    assert!(
        output.status.success(),
        "binary exited with non-zero code; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Hello from script!"),
        "expected 'Hello from script!' in stdout, got: {:?}",
        stdout
    );
}

/// A binary built with `--release` must also run and produce correct output.
#[test]
fn test_build_release_executable_produces_output() {
    let source_file = create_test_file(HELLO_WORLD);
    let out_dir = tempfile::tempdir().unwrap();
    let out_path_buf = out_dir.path().join("out_exe");
    let out_path = out_path_buf.as_path();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source_file.path())
        .arg("--out")
        .arg(out_path)
        .arg("--release")
        .assert()
        .success();

    let output = Command::new(out_path)
        .output()
        .expect("release binary should be executable");

    assert!(
        output.status.success(),
        "release binary exited non-zero; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Hello, Miri!"),
        "expected 'Hello, Miri!' in stdout, got: {:?}",
        stdout
    );
}

/// The built executable propagates the return value of `main` as the exit code.
///
/// `SIMPLE_MAIN` has `fn main() int` with body `42`, so the process must
/// exit with code 42. The body is NOT patched with `return 0` because the
/// function already declares an explicit return type.
#[test]
fn test_build_executable_exit_code_matches_main_return() {
    let source_file = create_test_file(SIMPLE_MAIN);
    let out_dir = tempfile::tempdir().unwrap();
    let out_path_buf = out_dir.path().join("out_exe");
    let out_path = out_path_buf.as_path();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source_file.path())
        .arg("--out")
        .arg(out_path)
        .assert()
        .success();

    let status = Command::new(out_path)
        .status()
        .expect("built binary should be executable");

    let code = status
        .code()
        .expect("process exited by signal unexpectedly");
    assert_eq!(code, 42, "expected main() return value 42 as exit code");
}
