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
        .stderr(predicates::str::contains("MER_CG_007"))
        .stderr(predicates::str::contains(
            "the LLVM backend is not available in this build of the compiler",
        ))
        .stderr(predicates::prelude::PredicateBooleanExt::not(
            predicates::str::contains("MER_CG_008"),
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

/// A loop of integer comparisons and branches whose cost is all in the code
/// the backend emits: no allocation, no runtime calls inside the loop, one
/// line of output. Unoptimized code materializes every comparison as a byte
/// and tests it again; optimized code branches on the comparison itself, which
/// is where the speed tier wins most on a loop like this one.
const TIGHT_LOOP: &str = r#"use system.io

fn classify(n int) int
    var counts = 0
    var i = 0
    while i < n
        let v = i & 255
        if v < 16
            counts += 1
        else if v < 64
            counts += 2
        else if v < 128
            counts += 3
        else if v < 200
            counts += 5
        else
            counts += 7
        i += 1
    counts

println(f"{classify(100000000)}")
"#;

/// What `classify(100000000)` prints, whatever the optimization level.
const TIGHT_LOOP_OUTPUT: &str = "412500000\n";

/// Build `source` with the extra `flags` and return the executable's path.
fn build_with_flags(
    source: &NamedTempFile,
    out_dir: &std::path::Path,
    flags: &[&str],
) -> std::path::PathBuf {
    let out_path = out_dir.join(format!("exe_{}", flags.join("_").replace('-', "")));
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out_path)
        .args(flags)
        .assert()
        .success();
    out_path
}

/// Run the executable at `path`, check what it printed, and return how long it
/// took.
fn timed_run(path: &std::path::Path) -> std::time::Duration {
    let start = std::time::Instant::now();
    let output = Command::new(path).output().expect("built binary runs");
    let elapsed = start.elapsed();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), TIGHT_LOOP_OUTPUT);
    elapsed
}

/// The fastest of a few runs: the suite runs in parallel, so a slow sample
/// measures the machine's load rather than the code.
fn best_run(path: &std::path::Path) -> std::time::Duration {
    (0..5).map(|_| timed_run(path)).min().unwrap()
}

/// Optimization level 3 reaches the backend: the code it emits is not the
/// code level 0 emits, and both compute the same answer.
#[test]
fn test_opt_level_3_emits_different_code_than_opt_level_0() {
    let source = create_test_file(TIGHT_LOOP);
    let out_dir = tempfile::tempdir().unwrap();
    let unoptimized = build_with_flags(&source, out_dir.path(), &["--opt-level", "0"]);
    let optimized = build_with_flags(&source, out_dir.path(), &["--opt-level", "3"]);

    let unoptimized_object = std::fs::read(unoptimized.with_extension("o")).unwrap();
    let optimized_object = std::fs::read(optimized.with_extension("o")).unwrap();
    assert!(
        unoptimized_object != optimized_object,
        "opt-level 3 emitted the same object code as opt-level 0"
    );
    timed_run(&unoptimized);
    timed_run(&optimized);
}

/// `--release` alone optimizes as `--opt-level 3` does, and an explicit
/// `--opt-level` beside it wins.
#[test]
fn test_release_implies_opt_level_3_unless_a_level_is_given() {
    let source = create_test_file(TIGHT_LOOP);
    let out_dir = tempfile::tempdir().unwrap();
    let object = |flags: &[&str]| {
        std::fs::read(build_with_flags(&source, out_dir.path(), flags).with_extension("o")).unwrap()
    };

    let release = object(&["--release"]);
    assert!(
        release == object(&["--release", "--opt-level", "3"]),
        "--release did not optimize as --opt-level 3"
    );
    assert!(
        release != object(&["--release", "--opt-level", "0"]),
        "--opt-level 0 beside --release was ignored"
    );
}

/// The optimized build of a tight arithmetic loop is measurably faster than
/// the unoptimized one. The bound is a generous ratio rather than a time: this
/// runs beside the rest of the suite on a loaded machine.
#[test]
fn test_opt_level_3_runs_a_tight_loop_faster_than_opt_level_0() {
    let source = create_test_file(TIGHT_LOOP);
    let out_dir = tempfile::tempdir().unwrap();
    let unoptimized = best_run(&build_with_flags(
        &source,
        out_dir.path(),
        &["--opt-level", "0"],
    ));
    let optimized = best_run(&build_with_flags(
        &source,
        out_dir.path(),
        &["--opt-level", "3"],
    ));

    assert!(
        optimized.as_secs_f64() < unoptimized.as_secs_f64() * 0.8,
        "opt-level 3 took {optimized:?}, opt-level 0 took {unoptimized:?}"
    );
}

/// `--emit mir` writes the program's MIR, as reference counting left it,
/// beside the executable; `--emit clif` writes the backend's IR of every
/// function. Neither changes the executable.
#[test]
fn test_build_emits_mir_and_clif_beside_the_executable() {
    let source = create_test_file(TIGHT_LOOP);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("loop");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(["--emit", "mir", "--emit", "clif"])
        .assert()
        .success();

    let mir = std::fs::read_to_string(out_dir.path().join("loop.mir")).unwrap();
    assert!(
        mir.contains("classify"),
        "the MIR names the program's function:\n{mir}"
    );
    let clif = std::fs::read_to_string(out_dir.path().join("loop.clif")).unwrap();
    assert!(
        clif.contains("function") && clif.contains("classify"),
        "the CLIF holds the function:\n{clif}"
    );
    timed_run(&out);

    let plain = build_with_flags(&source, out_dir.path(), &[]);
    assert!(
        std::fs::read(out.with_extension("o")).unwrap()
            == std::fs::read(plain.with_extension("o")).unwrap(),
        "--emit changed the object code"
    );
}

/// Without `--emit`, nothing but the build's own files is written.
#[test]
fn test_build_without_emit_writes_no_listing() {
    let source = create_test_file(SIMPLE_MAIN);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("main");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    assert!(!out_dir.path().join("main.mir").exists());
    assert!(!out_dir.path().join("main.clif").exists());
}
