// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The output-identity gate: every port of a workload prints the same bytes.
//!
//! A port that computes something else is not a data point, so before anything
//! is timed every port under `bench/workloads/<name>/` is built with its
//! charter build line, run on the manifest's parameter, and its stdout compared
//! with the checksum the manifest records. The Miri ports also run under the
//! leak check and the heap guard and are built with the MIR verifier on, and
//! without `--release`, which would leave those checks out: a benchmark
//! program that leaks is a bug report, not a result.
//!
//! Size S runs in the normal suite. `MIRI_BENCH_SIZES=s,m,l` runs more. A port
//! whose toolchain is not installed is skipped with a line naming the
//! toolchain, never passed silently.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The source file extension of each language's port. `rust-checked` builds
/// the Rust port again with Miri's checks on, so an overflow the release build
/// would wrap past fails the gate instead of skewing that arm.
const PORT_EXTENSIONS: [(&str, &str); 8] = [
    ("miri", "mi"),
    ("c", "c"),
    ("cpp", "cpp"),
    ("rust", "rs"),
    ("rust-checked", "rs"),
    ("go", "go"),
    ("python", "py"),
    ("typescript", "ts"),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse_toml(path: &Path) -> toml::Table {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.parse()
        .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()))
}

fn charter() -> toml::Table {
    parse_toml(&repo_root().join("bench").join("charter.toml"))
}

fn workload_dir(workload: &str) -> PathBuf {
    repo_root().join("bench").join("workloads").join(workload)
}

fn manifest(workload: &str) -> toml::Table {
    parse_toml(&workload_dir(workload).join("workload.toml"))
}

fn port_path(workload: &str, extension: &str) -> PathBuf {
    workload_dir(workload).join(format!("{workload}.{extension}"))
}

/// The sizes this run checks: S unless `MIRI_BENCH_SIZES` names others.
fn sizes_under_test() -> Vec<String> {
    std::env::var("MIRI_BENCH_SIZES")
        .unwrap_or_else(|_| "s".to_string())
        .split(',')
        .map(|size| size.trim().to_string())
        .collect()
}

/// A language's build or run line, as the charter spells it.
fn charter_line(charter: &toml::Table, language: &str, key: &str) -> Option<String> {
    charter["languages"][language]
        .get(key)
        .and_then(|line| line.as_str())
        .map(str::to_string)
}

/// The full path of `program`, or `None` when it is not installed. The
/// compiler under test is the one this suite built, never one on `PATH`.
fn find_program(program: &str) -> Option<PathBuf> {
    if program == "miri" {
        return Some(PathBuf::from(assert_cmd::cargo_bin!("miri")));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    })
}

/// A charter line as a command: placeholders filled in, leading `NAME=value`
/// words set in the child's environment, never interpreted by a shell. `None`
/// when the line's program is not installed.
fn command_for(line: &str, src: &Path, out: &Path) -> Option<Command> {
    let words: Vec<String> = line
        .split_whitespace()
        .map(|word| {
            word.replace("{src}", &src.to_string_lossy())
                .replace("{out}", &out.to_string_lossy())
        })
        .collect();
    let (assignments, rest): (Vec<&String>, Vec<&String>) = {
        let split = words
            .iter()
            .position(|word| !word.contains('=') || word.starts_with('-'))
            .unwrap_or(words.len());
        (
            words[..split].iter().collect(),
            words[split..].iter().collect(),
        )
    };
    let (program, arguments) = rest.split_first()?;
    let mut command = Command::new(find_program(program)?);
    command.args(arguments.iter().map(|word| word.as_str()));
    for assignment in assignments {
        if let Some((name, value)) = assignment.split_once('=') {
            command.env(name, value);
        }
    }
    Some(command)
}

/// The environment a Miri port is built and run under in this gate: the MIR
/// verifier and the leak check always, the heap guard when `heap_guard`.
fn with_miri_checks(command: &mut Command, heap_guard: bool) {
    command
        .env("MIRI_STDLIB_PATH", repo_root().join("src").join("stdlib"))
        .env("MIRI_VERIFY_MIR", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_HEAP_GUARD", if heap_guard { "1" } else { "0" })
        .env_remove("MIRI_CC")
        .env_remove("CC");
}

/// The line a Miri port is built with in this gate: the charter's, without
/// `--release`. A release build leaves the runtime checks out of the code it
/// emits, and this gate exists to run them; the optimization level stays the
/// one the charter times.
fn checked_build_line(line: &str) -> String {
    let words: Vec<&str> = line
        .split_whitespace()
        .filter(|word| *word != "--release")
        .collect();
    words.join(" ")
}

/// Build one port into `work_dir`, returning the executable its run line
/// names, or `None` when its toolchain is absent. A language that runs from
/// source builds nothing.
fn build_port(
    charter: &toml::Table,
    language: &str,
    src: &Path,
    work_dir: &Path,
) -> Option<PathBuf> {
    let out = work_dir.join(format!("{language}.out"));
    let Some(build_line) = charter_line(charter, language, "build") else {
        return Some(out);
    };
    let build_line = if language == "miri" {
        checked_build_line(&build_line)
    } else {
        build_line
    };
    let mut build = command_for(&build_line, src, &out)?;
    build.current_dir(work_dir);
    if language == "miri" {
        with_miri_checks(&mut build, false);
    }
    let output = build
        .output()
        .unwrap_or_else(|e| panic!("run {build_line}: {e}"));
    assert!(
        output.status.success(),
        "{language} port {} does not build:\n{}{}",
        src.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some(out)
}

/// The command that runs a built port on `input`, or `None` when the run
/// line's program is not installed.
fn run_command(
    charter: &toml::Table,
    language: &str,
    (src, out): (&Path, &Path),
    case: &Case,
) -> Option<Command> {
    let run_line = charter_line(charter, language, "run").expect("every language has a run line");
    let mut run = command_for(&run_line, src, out)?;
    run.arg(&case.input);
    if language == "miri" {
        with_miri_checks(&mut run, case.heap_guard);
    }
    Some(run)
}

/// Run one built port on `case` and assert it prints the case's checksum.
fn assert_port_output(
    charter: &toml::Table,
    (workload, language): (&str, &str),
    (src, out): (&Path, &Path),
    case: &Case,
) -> bool {
    let Some(mut run) = run_command(charter, language, (src, out), case) else {
        return false;
    };
    let output = run
        .output()
        .unwrap_or_else(|e| panic!("run the {language} port of {workload}: {e}"));
    assert!(
        output.status.success(),
        "the {language} port of {workload} failed at {}:\n{}",
        case.label,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", case.checksum),
        "the {language} port of {workload} printed a different checksum at {}",
        case.label
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("ignored: this program was built"),
        "the {language} port of {workload} ran without the checks this gate asks for:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

/// The program named first on a language's build or run line.
fn toolchain_of(charter: &toml::Table, language: &str) -> String {
    let line = charter_line(charter, language, "build")
        .or_else(|| charter_line(charter, language, "run"))
        .unwrap_or_default();
    line.split_whitespace()
        .find(|word| !word.contains('='))
        .unwrap_or_default()
        .to_string()
}

/// One run every port makes: an input file and the checksum it must print.
struct Case {
    label: String,
    input: PathBuf,
    checksum: String,
    /// Whether the Miri port runs under the heap guard for this case.
    heap_guard: bool,
}

/// The file a port reads for `entry` (a size, or the manifest's `[guard]`):
/// the parameter itself, or for a workload whose manifest says
/// `generated = true`, the text `bench/workloads/generate.py` writes for it,
/// checked against the SHA-256 the entry records. `None` when the generator's
/// interpreter is not installed.
fn prepare_input(
    (workload, manifest): (&str, &toml::Table),
    (label, entry): (&str, &toml::Value),
    work_dir: &Path,
) -> Option<PathBuf> {
    let parameter = entry["parameter"]
        .as_integer()
        .expect("parameter is an integer");
    let input = work_dir.join(format!("{label}.input"));
    if !manifest
        .get("generated")
        .and_then(|g| g.as_bool())
        .unwrap_or(false)
    {
        fs::write(&input, format!("{parameter}\n")).expect("write input");
        return Some(input);
    }
    let generator = repo_root()
        .join("bench")
        .join("workloads")
        .join("generate.py");
    let status = Command::new(find_program("python3")?)
        .arg(&generator)
        .arg(workload)
        .arg(parameter.to_string())
        .arg(&input)
        .status()
        .unwrap_or_else(|e| panic!("run {}: {e}", generator.display()));
    assert!(
        status.success(),
        "the generator failed for {workload} at {label}"
    );
    let bytes = fs::read(&input).expect("read the generated input");
    assert_eq!(
        hex_sha256(&bytes),
        entry["input_sha256"]
            .as_str()
            .expect("a generated input records its SHA-256"),
        "the generator wrote different input for {workload} at {label}"
    );
    Some(input)
}

/// The runs this pass makes: every size under test, and the `[guard]` run
/// when the manifest has one. Without one, the Miri port runs under the heap
/// guard at every size; with one, only there.
fn cases(workload: &str, manifest: &toml::Table, work_dir: &Path) -> Option<Vec<Case>> {
    let guard = manifest.get("guard");
    let mut entries: Vec<(String, &toml::Value, bool)> = sizes_under_test()
        .into_iter()
        .map(|size| {
            let entry = &manifest["sizes"][size.as_str()];
            (format!("size {size}"), entry, guard.is_none())
        })
        .collect();
    if let Some(guard) = guard {
        entries.push(("the heap-guard parameter".to_string(), guard, true));
    }
    entries
        .into_iter()
        .map(|(label, entry, heap_guard)| {
            let file_label = label.replace(' ', "-");
            let input = prepare_input((workload, manifest), (&file_label, entry), work_dir)?;
            let checksum = entry["checksum"]
                .as_str()
                .expect("checksum is text")
                .to_string();
            Some(Case {
                label,
                input,
                checksum,
                heap_guard,
            })
        })
        .collect()
}

/// Why the manifest says `language`'s port cannot run yet, if it does.
fn blocked_reason(manifest: &toml::Table, language: &str) -> Option<String> {
    manifest
        .get("blocked")
        .and_then(|blocked| blocked.get(language))
        .and_then(|reason| reason.as_str())
        .map(str::to_string)
}

/// Build and run every port of `workload` at every size under test, and
/// assert each prints exactly the manifest's checksum.
fn assert_identity(workload: &str) {
    let charter = charter();
    let manifest = manifest(workload);
    let work_dir = tempfile::tempdir().expect("scratch directory");
    let Some(cases) = cases(workload, &manifest, work_dir.path()) else {
        eprintln!("bench identity: skipping {workload}: `python3`, which writes its input, is not installed");
        return;
    };
    for (language, extension) in PORT_EXTENSIONS {
        if let Some(reason) = blocked_reason(&manifest, language) {
            eprintln!("bench identity: skipping the {language} port of {workload}: {reason}");
            continue;
        }
        let src = port_path(workload, extension);
        let ran = build_port(&charter, language, &src, work_dir.path()).is_some_and(|out| {
            cases
                .iter()
                .all(|case| assert_port_output(&charter, (workload, language), (&src, &out), case))
        });
        if !ran {
            eprintln!(
                "bench identity: skipping the {language} port of {workload}: `{}` is not installed",
                toolchain_of(&charter, language)
            );
        }
    }
}

/// The value of a port's `port-checklist` line and the SHA-256 of the file
/// without that line, which the line must name.
fn checklist_review(text: &str, comment: &str) -> Option<(String, String)> {
    let marker = format!("{comment} port-checklist: ");
    let line = text.lines().find(|line| line.starts_with(&marker))?;
    let without: String = text
        .split_inclusive('\n')
        .filter(|candidate| candidate.trim_end_matches('\n') != line)
        .collect();
    Some((
        line[marker.len()..].to_string(),
        hex_sha256(without.as_bytes()),
    ))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The file at `src` carries a review line, in `comment`'s syntax, bound to
/// its exact text.
fn assert_reviewed(src: &Path, comment: &str) {
    let text =
        fs::read_to_string(src).unwrap_or_else(|e| panic!("{} is missing: {e}", src.display()));
    let (review, hash) = checklist_review(&text, comment)
        .unwrap_or_else(|| panic!("{} has no port-checklist line", src.display()));
    assert!(
        review.starts_with("reviewed ") && review.contains(" by "),
        "{}: the port-checklist line names no date and reviewer: {review}",
        src.display()
    );
    assert!(
        review.ends_with(&format!("sha256={hash}")),
        "{}: the port-checklist line does not match the file; its text hashes to {hash}",
        src.display()
    );
}

/// Every port carries a review line bound to its exact text.
fn assert_ports_reviewed(workload: &str) {
    let charter = charter();
    for (language, extension) in PORT_EXTENSIONS {
        let comment = charter_line(&charter, language, "checklist_comment")
            .expect("every language names its comment marker");
        assert_reviewed(&port_path(workload, extension), &comment);
    }
}

/// Every manifest names the three sizes and a checksum for each.
fn assert_manifest_complete(workload: &str) {
    let manifest = manifest(workload);
    assert_eq!(manifest["name"].as_str(), Some(workload));
    let sizes: BTreeMap<_, _> = manifest["sizes"]
        .as_table()
        .expect("sizes is a table")
        .iter()
        .collect();
    assert_eq!(
        sizes.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
        ["l", "m", "s"]
    );
    for (size, entry) in sizes {
        assert!(
            entry["parameter"].is_integer(),
            "{workload} size {size} has no parameter"
        );
        assert!(
            entry["checksum"].is_str(),
            "{workload} size {size} has no checksum"
        );
    }
}

fn check_workload(workload: &str) {
    assert_manifest_complete(workload);
    assert_ports_reviewed(workload);
    assert_identity(workload);
}

#[test]
fn test_bench_identity_nbody() {
    check_workload("nbody");
}

#[test]
fn test_bench_identity_spectral_norm() {
    check_workload("spectral-norm");
}

#[test]
fn test_bench_identity_mandelbrot() {
    check_workload("mandelbrot");
}

#[test]
fn test_bench_identity_fannkuch_redux() {
    check_workload("fannkuch-redux");
}

#[test]
fn test_bench_identity_sieve() {
    check_workload("sieve");
}

#[test]
fn test_bench_identity_matmul() {
    check_workload("matmul");
}

#[test]
fn test_bench_identity_binary_trees() {
    check_workload("binary-trees");
}

#[test]
fn test_bench_identity_list_sort() {
    check_workload("list-sort");
}

#[test]
fn test_bench_identity_wordfreq() {
    check_workload("wordfreq");
}

#[test]
fn test_bench_identity_string_builder() {
    check_workload("string-builder");
}

#[test]
fn test_bench_identity_json_roundtrip() {
    check_workload("json-roundtrip");
}

#[test]
fn test_bench_identity_k_nucleotide() {
    check_workload("k-nucleotide");
}

#[test]
fn test_bench_identity_closures() {
    check_workload("closures");
}

#[test]
fn test_bench_identity_csv_aggregate() {
    check_workload("csv-aggregate");
}

#[test]
fn test_bench_identity_dispatch() {
    check_workload("dispatch");
}

#[test]
fn test_bench_identity_enum_vm() {
    check_workload("enum-vm");
}

#[test]
fn test_bench_identity_generics() {
    check_workload("generics");
}

#[test]
fn test_bench_identity_lz_compress() {
    check_workload("lz-compress");
}

#[test]
fn test_bench_identity_ray_tracer() {
    check_workload("ray-tracer");
}

/// The structures every C port shares are reviewed like a port.
#[test]
fn test_bench_identity_shared_c_code_is_reviewed() {
    assert_reviewed(&repo_root().join("bench").join("c").join("table.h"), "//");
}

/// Every workload directory is one the gate checks.
#[test]
fn test_bench_identity_covers_every_workload() {
    let checked = [
        "binary-trees",
        "closures",
        "csv-aggregate",
        "dispatch",
        "enum-vm",
        "fannkuch-redux",
        "generics",
        "json-roundtrip",
        "k-nucleotide",
        "list-sort",
        "lz-compress",
        "mandelbrot",
        "matmul",
        "nbody",
        "ray-tracer",
        "sieve",
        "spectral-norm",
        "string-builder",
        "wordfreq",
    ];
    let mut present: Vec<String> = fs::read_dir(repo_root().join("bench").join("workloads"))
        .expect("bench/workloads exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    present.sort();
    assert_eq!(present, checked);
}
