// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::collections::HashMap;
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

/// `--profile` builds the code `--release` builds, down to the backend's IR,
/// and keeps a frame pointer in every function: AArch64 otherwise leaves the
/// frame out of a leaf such as `classify`, so there the object code differs.
#[test]
fn test_profile_builds_the_release_code_with_frame_pointers() {
    let source = create_test_file(TIGHT_LOOP);
    let out_dir = tempfile::tempdir().unwrap();
    let build = |flags: &[&str]| build_with_flags(&source, out_dir.path(), flags);
    let read = |path: &std::path::Path, extension: &str| {
        let mut file = path.as_os_str().to_owned();
        file.push(extension);
        std::fs::read(file).unwrap()
    };

    let profile = build(&["--profile", "--emit", "clif"]);
    let release = build(&["--release", "--emit", "clif"]);
    assert!(
        read(&profile, ".clif") == read(&release, ".clif"),
        "--profile did not build the release code"
    );
    if cfg!(target_arch = "aarch64") {
        assert!(
            read(&profile, ".o") != read(&release, ".o"),
            "--profile left the frame out of the AArch64 leaf"
        );
    }
    timed_run(&profile);
}

/// The optimized build of a tight loop is measurably faster than the
/// unoptimized one. The bound is a generous ratio rather than a time: this runs
/// beside the rest of the suite on a loaded machine, so the two builds are run
/// in turn and each keeps its fastest run, which load affects alike.
///
/// The loop's gain comes from branching on a comparison instead of
/// materializing it as a byte and testing it again, which Cranelift does only
/// when optimizing on AArch64: there the optimized build takes about 0.64 of
/// the unoptimized one's time. Its x86-64 lowering branches on the comparison
/// even unoptimized (measured: 674 ms against 683 ms), so there the two builds
/// take the same time and a shared runner's load alone decides which is
/// faster; the different object code is what the test above proves there.
#[test]
#[cfg_attr(
    not(target_arch = "aarch64"),
    ignore = "the optimizer does not speed this loop up outside AArch64"
)]
fn test_opt_level_3_runs_a_tight_loop_faster_than_opt_level_0() {
    let source = create_test_file(TIGHT_LOOP);
    let out_dir = tempfile::tempdir().unwrap();
    let unoptimized_exe = build_with_flags(&source, out_dir.path(), &["--opt-level", "0"]);
    let optimized_exe = build_with_flags(&source, out_dir.path(), &["--opt-level", "3"]);
    let mut unoptimized = std::time::Duration::MAX;
    let mut optimized = std::time::Duration::MAX;
    for _ in 0..7 {
        unoptimized = unoptimized.min(timed_run(&unoptimized_exe));
        optimized = optimized.min(timed_run(&optimized_exe));
    }
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

/// A generic class's methods are compiled at the instantiations the program
/// reaches; the open template, written in the class's own parameters, is
/// compiled only where something names it, and here nothing does.
#[test]
fn test_an_unreferenced_open_class_template_is_not_emitted() {
    let source = create_test_file(concat!(
        "class Box<U>\n",
        "    var item U?\n",
        "    fn getter() fn() U?\n",
        "        let v = self.item\n",
        "        return fn() U?: v\n",
        "\n",
        "fn main()\n",
        "    let b = Box<int>(item: 3)\n",
        "    println(f\"{b.getter()() ?? -1}\")\n",
    ));
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("boxed");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(["--emit", "mir"])
        .assert()
        .success();

    let mir = std::fs::read_to_string(out_dir.path().join("boxed.mir")).unwrap();
    let functions: Vec<&str> = mir
        .lines()
        .filter_map(|line| line.strip_prefix("fn "))
        .collect();
    assert!(
        functions.iter().any(|name| name.starts_with("miri.Box$")),
        "the instantiation is compiled: {functions:?}"
    );
    assert!(
        !functions
            .iter()
            .any(|name| *name == "miri.Box.getter"
                || name.contains("lambda") && name.ends_with("$Box")),
        "the open template and its lambda are not: {functions:?}"
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

const FIELD_WRITE: &str = r#"
use system.io
use system.collections.list

struct Body
    x float
    vx float

fn write(start [Body], i int, v float) [Body]
    var bodies = start
    bodies[i].x = v
    bodies

fn main()
    let b = List([Body(x: 1.0, vx: 2.0)])
    let c = write(b, 0, 3.0)
    println(f"{b[0].x} {c[0].x}")
"#;

/// The blocks of one function in a CLIF listing, as (label, text) pairs, and
/// the name the listing gives each `fnN`/`gvN` reference.
fn clif_function<'a>(
    listing: &'a str,
    name: &str,
) -> (Vec<(&'a str, String)>, HashMap<&'a str, &'a str>) {
    let header = format!("; {name}\n");
    let start = listing
        .find(&header)
        .expect("the function is in the listing")
        + header.len();
    let mut blocks: Vec<(&str, String)> = Vec::new();
    let mut names = HashMap::new();
    for line in listing[start..].lines() {
        let trimmed = line.trim();
        if let Some(reference) = line.strip_prefix("; ") {
            match reference.split_once(" = ") {
                Some((handle, target)) => names.insert(handle, target),
                None => break,
            };
        } else if line.starts_with("block") {
            let label = line.split(['(', ':']).next().unwrap_or(line);
            blocks.push((label, String::new()));
        } else if let Some((_, body)) = blocks.last_mut() {
            body.push_str(trimmed);
            body.push('\n');
        }
    }
    (blocks, names)
}

/// In a build that keeps the runtime checks, a write into a list element asks
/// whether the list is shared by reading its count inline; the runtime is
/// called for it only on the path taken when the heap guard or another
/// tracking mode is on, so a hot loop of field writes makes no call for the
/// question.
#[test]
fn test_the_sharing_check_calls_the_runtime_only_when_tracking() {
    let source = create_test_file(FIELD_WRITE);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("write");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(["--opt-level", "3", "--emit", "clif"])
        .assert()
        .success();
    let listing = std::fs::read_to_string(out_dir.path().join("write.clif")).unwrap();
    let (blocks, names) = clif_function(&listing, "miri.write");
    let handle_of = |target: &str| {
        names
            .iter()
            .filter(|(_, name)| **name == target)
            .map(|(handle, _)| *handle)
            .collect::<Vec<_>>()
    };
    let checks = handle_of("miri_rt_rc_is_shared");
    let tracking = handle_of("miri_rt_tracking_state");
    let calling: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| checks.iter().any(|f| body.contains(&format!("call {f}("))))
        .map(|(label, _)| *label)
        .collect();
    assert!(
        !checks.is_empty(),
        "the write still has a sharing check:\n{listing}"
    );
    for label in calling {
        let entered_from: Vec<&String> = blocks
            .iter()
            .filter(|(_, body)| {
                body.lines().any(|l| {
                    (l.starts_with("brif") || l.starts_with("jump"))
                        && l.split([' ', ',', '(']).any(|word| word == label)
                })
            })
            .map(|(_, body)| body)
            .collect();
        assert!(
            !entered_from.is_empty()
                && entered_from.iter().all(|body| {
                    tracking
                        .iter()
                        .any(|gv| body.contains(&format!("symbol_value.i64 {gv}")))
                }),
            "{label} calls the runtime for the sharing check outside a tracking branch:\n{listing}"
        );
    }
    let run = Command::new(&out).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "1.0 3.0",
        "value semantics"
    );
}

const CHECKED_HEAP_WORK: &str = r#"
use system.io
use system.collections.list

struct Body
    x float
    vx float

class Counter
    var hits int

    fn init()
        self.hits = 0

fn write(start [Body], i int, v float) [Body]
    var bodies = start
    bodies[i].x = v
    bodies

fn count(n int) int
    var total = 0
    var i = 0
    while i < n
        var c = Counter()
        c.hits = i
        total = total + c.hits
        i = i + 1
    total

fn main()
    let b = List([Body(x: 1.0, vx: 2.0)])
    let c = write(b, 0, 3.0)
    let counters = List([Counter(), Counter()])
    println(f"{b[0].x} {c[0].x} {count(4)} {counters.length()}")
"#;

/// The runtime symbols compiled code reaches only to serve a check or a
/// counter: the tracking state it tests and the hooks behind that test.
const CHECK_HOOKS: [&str; 7] = [
    "miri_rt_tracking_state",
    "miri_rt_rc_is_shared",
    "miri_rt_class_alloc_track",
    "miri_rt_class_free_track",
    "miri_rt_release_check",
    "miri_rt_rc_stats_dup",
    "miri_rt_rc_stats_free",
];

/// Builds `CHECKED_HEAP_WORK` with `flags` and the CLIF listing, returning the
/// executable and the listing.
fn build_heap_work(flags: &[&str], out_dir: &std::path::Path) -> (std::path::PathBuf, String) {
    let source = create_test_file(CHECKED_HEAP_WORK);
    let out = out_dir.join("heap");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(flags)
        .args(["--emit", "clif"])
        .assert()
        .success();
    let listing = std::fs::read_to_string(out_dir.join("heap.clif")).unwrap();
    (out, listing)
}

/// The check hooks a function's CLIF listing references, by name.
fn referenced_check_hooks(listing: &str, function: &str) -> Vec<String> {
    let (_, names) = clif_function(listing, function);
    let mut hooks: Vec<String> = names
        .values()
        .filter(|name| CHECK_HOOKS.contains(name))
        .map(|name| name.to_string())
        .collect();
    hooks.sort();
    hooks.dedup();
    hooks
}

/// The functions a CLIF listing holds, by the name each is headed with.
fn listed_functions(listing: &str) -> Vec<&str> {
    let lines: Vec<&str> = listing.lines().collect();
    lines
        .windows(2)
        .filter(|pair| pair[1].starts_with("function "))
        .filter_map(|pair| pair[0].strip_prefix("; "))
        .collect()
}

/// A release build leaves the runtime checks out of the code it emits: no
/// compiled function loads the tracking state or reaches a hook behind it, so
/// no call is left on a path through a loop for a check that is not running.
/// Only the type thunks keep theirs, and the runtime holds them off from the
/// entry point's first call. The same program built without `--release` keeps
/// every hook.
#[test]
fn test_a_release_build_emits_no_runtime_check_hooks() {
    let out_dir = tempfile::tempdir().unwrap();
    let (out, listing) = build_heap_work(&["--release"], out_dir.path());
    let functions = listed_functions(&listing);
    for function in ["miri.write", "miri.count", "main"] {
        assert!(functions.contains(&function), "{function} is not listed");
    }
    for function in functions.iter().filter(|name| !name.contains('$')) {
        assert_eq!(
            referenced_check_hooks(&listing, function),
            Vec::<String>::new(),
            "{function} still reaches a check hook:\n{listing}"
        );
    }
    let run = Command::new(&out).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "1.0 3.0 6 2");

    let checked_dir = tempfile::tempdir().unwrap();
    let (_, checked) = build_heap_work(&["--opt-level", "3"], checked_dir.path());
    assert!(
        referenced_check_hooks(&checked, "miri.count").contains(&"miri_rt_tracking_state".into()),
        "a build without --release lost its check hooks:\n{checked}"
    );
}

/// The first call a release build's entry point makes tells the runtime its
/// checks are compiled out, before any statement of the program runs; a
/// build that keeps them makes no such call.
#[test]
fn test_a_release_entry_point_first_reports_its_checks_compiled_out() {
    let notice = "miri_rt_checks_compiled_out";
    let first_call = |listing: &str| {
        let (blocks, names) = clif_function(listing, "main");
        let entry = &blocks.first().expect("main has a block").1;
        let call = entry
            .lines()
            .find(|line| line.contains("call fn"))
            .expect("main makes a call");
        let handle = call
            .split("call ")
            .nth(1)
            .unwrap()
            .split('(')
            .next()
            .unwrap();
        names[handle].to_string()
    };
    let out_dir = tempfile::tempdir().unwrap();
    let (_, release) = build_heap_work(&["--release"], out_dir.path());
    assert_eq!(first_call(&release), notice);

    let checked_dir = tempfile::tempdir().unwrap();
    let (_, checked) = build_heap_work(&["--opt-level", "3"], checked_dir.path());
    assert!(
        !checked.contains(notice),
        "a checked build reports its checks compiled out"
    );
}

/// The line a release program prints when started with the heap guard and the
/// leak check asked for.
const GUARD_AND_LEAK_IGNORED: &str = "miri: MIRI_HEAP_GUARD, MIRI_LEAK_CHECK ignored: this \
     program was built with --release, which leaves the runtime checks out; build it \
     without --release to use them";

/// A release binary started with checks asked for runs unchecked and says so
/// once on stderr. It neither ignores the request silently nor reports on the
/// half of the heap its runtime sees: the program frees class instances its
/// compiled code allocated through a list the runtime releases, which no
/// leak count could balance, and it still exits 0 with no leak report.
#[test]
fn test_a_release_binary_names_the_checks_it_cannot_run() {
    let out_dir = tempfile::tempdir().unwrap();
    let (out, _) = build_heap_work(&["--release"], out_dir.path());
    let run = Command::new(&out)
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env_remove("MIRI_ALLOC_COUNT")
        .env_remove("MIRI_RC_STATS")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "release binary failed: {stderr}");
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "1.0 3.0 6 2");
    assert_eq!(stderr.trim(), GUARD_AND_LEAK_IGNORED);
}

/// The same program built without `--release` runs the checks it is asked for.
#[test]
fn test_a_checked_build_counts_what_it_is_asked_to() {
    let out_dir = tempfile::tempdir().unwrap();
    let (checked, _) = build_heap_work(&["--opt-level", "3"], out_dir.path());
    let run = Command::new(&checked)
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_ALLOC_COUNT", "1")
        .env_remove("MIRI_LEAK_CHECK")
        .env_remove("MIRI_RC_STATS")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "checked binary failed: {stderr}");
    assert!(
        stderr.starts_with("MIRI_ALLOC_COUNT:") && !stderr.contains("--release"),
        "the checked build did not count as asked: {stderr}"
    );
}

/// A script's entry point is synthesized around its top-level statements, and
/// `miri run --release` builds through the same path as `miri build`: both
/// report their checks compiled out.
#[test]
fn test_a_released_script_run_names_the_checks_it_cannot_run() {
    let source = create_test_file(HELLO_WORLD_SCRIPT);
    let run = miri_cmd()
        .arg("run")
        .arg(source.path())
        .arg("--release")
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "released script failed: {stderr}");
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("Hello from script!"),
        "stdout: {}",
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(stderr.contains(GUARD_AND_LEAK_IGNORED), "stderr: {stderr}");
}

/// `miri run --profile` builds in release mode, so a check asked for is
/// reported as compiled out.
#[test]
fn test_a_profiled_script_run_leaves_the_runtime_checks_out() {
    let source = create_test_file(HELLO_WORLD_SCRIPT);
    let run = miri_cmd()
        .arg("run")
        .arg(source.path())
        .arg("--profile")
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "profiled script failed: {stderr}");
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("Hello from script!"),
        "stdout: {}",
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(stderr.contains(GUARD_AND_LEAK_IGNORED), "stderr: {stderr}");
}

/// A program reaching `miri_rt_hash_combine`, a runtime function that makes
/// no calls: Rust leaves the frame out of it unless the runtime is built with
/// frame pointers, so its first instructions say which runtime was linked.
const HASH_COMBINE: &str = r#"use system.io
use system.ops

println(f"{hash_combine(1, 2)}")
"#;

/// The first two instructions of `name` in the executable at `path`, as
/// `objdump` disassembles them, or `None` when `objdump` is not installed.
fn first_instructions(path: &std::path::Path, name: &str) -> Option<[String; 2]> {
    let listing = Command::new("objdump").arg("-d").arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&listing.stdout);
    let headers = [format!("<{name}>:"), format!("<_{name}>:")];
    let mut lines = text.lines();
    lines
        .find(|line| headers.iter().any(|header| line.ends_with(header.as_str())))
        .unwrap_or_else(|| panic!("the executable should define {name}"));
    let mut instructions = lines
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string);
    let first = instructions.next().unwrap_or_default();
    let second = instructions.next().unwrap_or_default();
    Some([first, second])
}

/// Whether two disassembled instructions set up a frame record: `stp x29,
/// x30, ...` then `mov x29, sp` on AArch64, `push %rbp` then `mov %rsp,%rbp`
/// on x86-64.
fn is_frame_setup([first, second]: &[String; 2]) -> bool {
    let second = second.replace(' ', "");
    if cfg!(target_arch = "aarch64") {
        first.contains("stp") && first.contains("x29, x30") && second.contains("x29,sp")
    } else {
        first.contains("push")
            && first.contains("%rbp")
            && second.contains("mov")
            && second.contains("%rsp,%rbp")
    }
}

/// `--profile` links the core runtime built with frame pointers. On AArch64,
/// where rustc leaves the frame out of a function that makes no calls, the
/// `--release` build is also checked to link the standard runtime; on x86-64
/// rustc may push `rbp` there as an ordinary callee-saved register, so only
/// the profile build's full frame setup is checked.
#[test]
fn test_only_a_profile_build_links_the_frame_pointer_runtime() {
    let source = create_test_file(HASH_COMBINE);
    let out_dir = tempfile::tempdir().unwrap();
    let prologue = |flags: &[&str]| {
        let exe = build_with_flags(&source, out_dir.path(), flags);
        first_instructions(&exe, "miri_rt_hash_combine")
    };
    let Some(profile) = prologue(&["--profile"]) else {
        println!("skipping: objdump is not on PATH, so the linked runtime was not inspected");
        return;
    };
    assert!(
        is_frame_setup(&profile),
        "--profile linked a runtime without frame pointers: {profile:?}"
    );
    if cfg!(target_arch = "aarch64") {
        let release = prologue(&["--release"]).expect("objdump ran for the profile build");
        assert!(
            !is_frame_setup(&release),
            "--release linked the frame-pointer runtime: {release:?}"
        );
    }
}

/// A compiler with no frame-pointer runtime beside it refuses a profile build
/// and says how to make one, rather than linking the runtime without frame
/// pointers; a release build from the same place still links.
#[test]
fn test_a_profile_build_without_the_frame_pointer_runtime_is_refused() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = tempfile::tempdir().unwrap();
    let compiler = bin_dir.path().join("miri");
    std::fs::copy(assert_cmd::cargo_bin!("miri"), &compiler).unwrap();
    let source = create_test_file(HASH_COMBINE);
    let build = |flag: &str| {
        Command::new(&compiler)
            .arg("build")
            .arg(source.path())
            .arg("--out")
            .arg(bin_dir.path().join(flag.trim_start_matches('-')))
            .arg(flag)
            .current_dir(bin_dir.path())
            .env(
                "MIRI_RUNTIME_DIR",
                manifest.join("src/runtime/core/target/release"),
            )
            .env("MIRI_STDLIB_PATH", manifest.join("src/stdlib"))
            .output()
            .unwrap()
    };

    let release = build("--release");
    assert!(
        release.status.success(),
        "{}",
        String::from_utf8_lossy(&release.stderr)
    );
    let profile = build("--profile");
    let stderr = String::from_utf8_lossy(&profile.stderr);
    assert!(!profile.status.success(), "the profile build linked");
    assert!(
        stderr.contains("frame pointers") && stderr.contains("make runtimes"),
        "stderr: {stderr}"
    );
}

/// A copy of the compiler in a temporary directory that also holds
/// `src/runtime/core/target`, where the compiler looks for its runtimes.
struct RuntimeLayout {
    root: tempfile::TempDir,
    compiler: std::path::PathBuf,
}

impl RuntimeLayout {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let compiler = bin.join("miri");
        std::fs::copy(assert_cmd::cargo_bin!("miri"), &compiler).unwrap();
        RuntimeLayout { root, compiler }
    }

    /// Create `src/runtime/core/target/<relative>`, holding the core
    /// staticlib when `library` is given, written with that modification time.
    fn runtime_dir(&self, relative: &str, library: Option<std::time::SystemTime>) {
        let dir = self
            .root
            .path()
            .join("src/runtime/core/target")
            .join(relative);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(modified) = library {
            let file = std::fs::File::create(dir.join("libmiri_runtime_core.a")).unwrap();
            file.set_modified(modified).unwrap();
        }
    }

    /// The stderr of a `--profile` build of `source`, which must fail.
    fn refused_profile_build(&self, source: &NamedTempFile) -> String {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = Command::new(&self.compiler)
            .arg("build")
            .arg(source.path())
            .arg("--out")
            .arg(self.root.path().join("profiled"))
            .arg("--profile")
            .current_dir(self.root.path())
            .env("MIRI_STDLIB_PATH", manifest.join("src/stdlib"))
            .output()
            .unwrap();
        assert!(!output.status.success(), "the profile build linked");
        String::from_utf8_lossy(&output.stderr).into_owned()
    }
}

/// A frame-pointer directory with no staticlib in it is reported as a missing
/// runtime, with the command that builds it, before the linker is run.
#[test]
fn test_a_frame_pointer_directory_without_its_library_is_refused() {
    let layout = RuntimeLayout::new();
    layout.runtime_dir("frame-pointers/release", None);
    let stderr = layout.refused_profile_build(&create_test_file(HASH_COMBINE));
    assert!(
        stderr.contains("frame pointers") && stderr.contains("make runtimes"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("Linker failed"), "stderr: {stderr}");
}

/// A frame-pointer copy older than the standard runtime beside it is stale
/// code, so a profile build refuses it rather than profiling the old runtime.
#[test]
fn test_a_frame_pointer_library_older_than_the_standard_one_is_refused() {
    let layout = RuntimeLayout::new();
    let now = std::time::SystemTime::now();
    layout.runtime_dir(
        "frame-pointers/release",
        Some(now - std::time::Duration::from_secs(3600)),
    );
    layout.runtime_dir("release", Some(now));
    let stderr = layout.refused_profile_build(&create_test_file(HASH_COMBINE));
    assert!(
        stderr.contains("older than the standard runtime") && stderr.contains("make runtimes"),
        "stderr: {stderr}"
    );
}

const STRIDED_WRITES: &str = r#"
use system.io
use system.collections.list

fn mark(n int) int
    var flags = List<bool>()
    var i = 0
    while i < n
        flags.push(false)
        i = i + 1
    var p = 2
    while p * p < n
        var m = p * p
        while m < n
            flags[m] = true
            m = m + p
        p = p + 1
    var kept = flags
    kept[0] = true
    var count = 0
    i = 0
    while i < n
        if not flags[i]
            count = count + 1
        i = i + 1
    count

fn main()
    println(f"{mark(100)}")
"#;

/// Builds `STRIDED_WRITES` with `flags`, returning the executable, the blocks
/// of `miri.mark` that call `miri_rt_list_cow`, and for each the text of every
/// block that branches or jumps to it.
fn copy_on_write_callers(
    flags: &[&str],
    out_dir: &std::path::Path,
) -> (std::path::PathBuf, Vec<Vec<String>>, String) {
    let source = create_test_file(STRIDED_WRITES);
    let out = out_dir.join("strided");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(flags)
        .args(["--emit", "clif"])
        .assert()
        .success();
    let listing = std::fs::read_to_string(out_dir.join("strided.clif")).unwrap();
    let (blocks, names) = clif_function(&listing, "miri.mark");
    let handle_of = |target: &str| {
        names
            .iter()
            .filter(|(_, name)| **name == target)
            .map(|(handle, _)| handle.to_string())
            .collect::<Vec<_>>()
    };
    let cow = handle_of("miri_rt_list_cow");
    let tracking = handle_of("miri_rt_tracking_state");
    let entered_from = |label: &str| -> Vec<String> {
        blocks
            .iter()
            .filter(|(_, body)| {
                body.lines().any(|l| {
                    (l.starts_with("brif") || l.starts_with("jump"))
                        && l.split([' ', ',', '(']).any(|word| word == label)
                })
            })
            .map(|(_, body)| {
                // Name the tracking state so a caller can tell its test apart
                // from the count compare.
                tracking.iter().fold(body.clone(), |text, gv| {
                    text.replace(gv.as_str(), "TRACKING")
                })
            })
            .collect()
    };
    let callers = blocks
        .iter()
        .filter(|(_, body)| cow.iter().any(|f| body.contains(&format!("call {f}("))))
        .map(|(label, _)| entered_from(label))
        .collect();
    (out, callers, listing)
}

/// Whether `block` compares the count in the word before a payload with 1.
fn compares_the_count(block: &str) -> bool {
    let loaded: Vec<&str> = block
        .lines()
        .filter(|line| line.contains("= load.i64") && line.ends_with("-8"))
        .filter_map(|line| line.split(" = ").next())
        .collect();
    loaded.iter().any(|count| {
        block.lines().any(|line| {
            line.contains("icmp") && line.contains(" ne ") && line.contains(&format!("{count},"))
        })
    })
}

/// A write into a list held by a local reaches the runtime's copy-on-write
/// entry point only past an inline compare of the list's count with 1: a list
/// only one binding holds goes straight to the write. The values are
/// unchanged: the write into `kept` copies, so `flags` keeps its first element
/// unmarked and counts 27 (0, 1 and the 25 primes below 100) rather than 26.
#[test]
fn test_a_list_write_reaches_the_copy_on_write_runtime_only_past_a_count_compare() {
    let out_dir = tempfile::tempdir().unwrap();
    let (out, callers, listing) = copy_on_write_callers(&["--release"], out_dir.path());
    assert!(
        !callers.is_empty(),
        "the writes lost their copy-on-write call:\n{listing}"
    );
    for entered_from in &callers {
        assert!(
            !entered_from.is_empty() && entered_from.iter().all(|block| compares_the_count(block)),
            "a copy-on-write call is reached without the count compare:\n{listing}"
        );
    }
    let run = Command::new(&out).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "27");
}

/// In a build that keeps the runtime checks, the copy-on-write entry point is
/// also reached while the runtime is tracking, so the heap guard validates the
/// list before a write; it is reached from the count compare or from the test
/// of the tracking state, and from nothing else.
#[test]
fn test_a_checked_build_reaches_copy_on_write_while_tracking() {
    let out_dir = tempfile::tempdir().unwrap();
    let (out, callers, listing) = copy_on_write_callers(&["--opt-level", "3"], out_dir.path());
    assert!(
        !callers.is_empty(),
        "the writes lost their copy-on-write call:\n{listing}"
    );
    for entered_from in &callers {
        assert!(
            entered_from.iter().any(|block| block.contains("TRACKING"))
                && entered_from
                    .iter()
                    .all(|block| block.contains("TRACKING") || compares_the_count(block)),
            "a copy-on-write call is reached past neither test:\n{listing}"
        );
    }
    let run = Command::new(&out)
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "27");
}

const FILL: &str = r#"
use system.io
use system.collections.list

fn fill(n int) int
    var l = List<int>()
    var i = 0
    while i < n
        l.push(i)
        i = i + 1
    var total = 0
    i = 0
    while i < l.length()
        total = total + l[i]
        i = i + 1
    total

fn main()
    println(f"{fill(100)}")
"#;

/// A list's length is read from its header rather than through the method
/// and a runtime call, and a push reaches the runtime only past an inline
/// compare of the length with the capacity: a list with room takes the
/// element without a call.
#[test]
fn test_a_list_fill_loop_reads_the_length_inline_and_pushes_without_a_call_while_room_remains() {
    let source = create_test_file(FILL);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("fill");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(["--release", "--emit", "clif"])
        .assert()
        .success();
    let listing = std::fs::read_to_string(out_dir.path().join("fill.clif")).unwrap();
    let (blocks, names) = clif_function(&listing, "miri.fill");
    let referenced: Vec<&&str> = names.values().collect();
    for gone in ["miri.List.length", "miri_rt_list_len"] {
        assert!(
            !referenced.contains(&&gone),
            "fill still calls {gone}:\n{listing}"
        );
    }
    let push: Vec<&str> = names
        .iter()
        .filter(|(_, name)| **name == "miri_rt_list_push")
        .map(|(handle, _)| *handle)
        .collect();
    let calling: Vec<&str> = blocks
        .iter()
        .filter(|(_, body)| push.iter().any(|f| body.contains(&format!("call {f}("))))
        .map(|(label, _)| *label)
        .collect();
    assert!(!calling.is_empty(), "fill lost its push:\n{listing}");
    for label in calling {
        let checks_room = blocks.iter().any(|(_, body)| {
            body.contains("icmp ult")
                && body.lines().any(|l| {
                    l.starts_with("brif") && l.split([' ', ',', '(']).any(|word| word == label)
                })
        });
        assert!(
            checks_room,
            "{label} pushes without checking for room:\n{listing}"
        );
    }
    let run = Command::new(&out).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "4950");
}

/// Every call into a Miri runtime library is a direct branch: the libraries
/// are linked into the executable, so the callee is declared colocated rather
/// than reached through the GOT. Compiled code calls nothing else: even its
/// heap blocks come from the runtime, not from the C library's `malloc`.
#[test]
fn test_calls_into_the_runtime_are_direct_and_nothing_else_is_called() {
    let out_dir = tempfile::tempdir().unwrap();
    let (_, listing) = build_heap_work(&["--release"], out_dir.path());
    let mut runtime_calls = 0;
    for function in listed_functions(&listing) {
        let (_, names) = clif_function(&listing, function);
        let header = &listing[listing.find(&format!("; {function}\n")).unwrap()..];
        for (handle, name) in &names {
            let Some(declaration) = header
                .lines()
                .find(|line| line.trim_start().starts_with(&format!("{handle} = ")))
            else {
                continue;
            };
            if !handle.starts_with("fn") {
                continue;
            }
            // A Miri function is linked as `miri.` or `miri$` followed by its
            // path; anything else must be a runtime function.
            let is_miri_function = name.starts_with("miri.") || name.starts_with("miri$");
            assert!(
                name.starts_with("miri_rt_") || is_miri_function,
                "{function} calls {name}, outside the runtime: {declaration}"
            );
            if name.starts_with("miri_rt_") {
                runtime_calls += 1;
                assert!(
                    declaration.contains("colocated"),
                    "{function} calls {name} through the GOT: {declaration}"
                );
            }
        }
    }
    assert!(runtime_calls > 0, "nothing was checked:\n{listing}");
}

const CONSTANT_DIVISOR: &str = r#"
use system.io

fn half(x int) int
    x / 2

fn tail(x int) int
    x % 10

fn ratio(x int, y int) int
    x / y

fn main()
    println(f"{half(-7)} {tail(-7)} {ratio(9, 2)}")
"#;

/// The panics a function's CLIF listing references, by name.
fn referenced_panics(listing: &str, function: &str) -> Vec<String> {
    let (_, names) = clif_function(listing, function);
    let mut panics: Vec<String> = names
        .values()
        .filter(|name| name.ends_with("_panic"))
        .map(|name| name.to_string())
        .collect();
    panics.sort();
    panics
}

/// Dividing by a constant other than 0 and -1 can neither divide by zero nor
/// overflow, so no check for either is emitted; a divisor read at run time
/// keeps both, and the results are unchanged.
#[test]
fn test_a_constant_divisor_emits_no_division_checks() {
    let source = create_test_file(CONSTANT_DIVISOR);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("divide");
    miri_cmd()
        .arg("build")
        .arg(source.path())
        .arg("--out")
        .arg(&out)
        .args(["--release", "--emit", "clif"])
        .assert()
        .success();
    let listing = std::fs::read_to_string(out_dir.path().join("divide.clif")).unwrap();
    assert_eq!(
        referenced_panics(&listing, "miri.half"),
        Vec::<String>::new()
    );
    assert_eq!(
        referenced_panics(&listing, "miri.tail"),
        Vec::<String>::new()
    );
    assert_eq!(
        referenced_panics(&listing, "miri.ratio"),
        vec![
            "miri_rt_div_by_zero_panic".to_string(),
            "miri_rt_integer_overflow_panic".to_string()
        ]
    );
    let run = Command::new(&out).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "-3 -7 4");
}
