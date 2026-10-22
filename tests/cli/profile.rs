// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A program that spends nearly all its time on line 8, the loop body's
/// first statement.
const HOT_LOOP: &str = r#"use system.io

fn spin(n int) int
    var acc = 0
    var i = 0
    while i < n
        i += 1
        acc = (acc * 31 + i) % 1000003
    acc

fn main()
    println(f"{spin(100000000)}")
"#;

/// The line of `HOT_LOOP` its time goes to.
const HOT_LINE: usize = 8;

/// The lines of `HOT_LOOP`'s loop: `var i`, the `while`, and its body.
const LOOP_LINES: [usize; 4] = [5, 6, 7, 8];

/// The symbol a profile build's function table is defined under.
const FUNCTION_TABLE: &[u8] = b"miri_profile_functions";

/// Write `source` as `prog.mi` in `dir` and build it with `flags` to
/// `dir/out/prog`, returning the executable's path.
fn build(dir: &Path, source: &str, flags: &[&str]) -> PathBuf {
    std::fs::write(dir.join("prog.mi"), source).unwrap();
    let out = dir.join("out").join("prog");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    miri_cmd()
        .current_dir(dir)
        .arg("build")
        .arg("prog.mi")
        .arg("--out")
        .arg(&out)
        .args(flags)
        .assert()
        .success();
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The program's executable, run with `MIRI_PROFILE_OUT` as `record` asks.
fn run(executable: &Path, record: Option<&Path>) -> std::process::Output {
    let mut command = Command::new(executable);
    // A profile build is a release build, which notes on stderr that it
    // ignores the runtime checks; a job that arms them for the whole suite
    // must not reach these runs, whose stderr is asserted empty.
    command
        .env_remove("MIRI_PROFILE_OUT")
        .env_remove("MIRI_PROFILE_HZ")
        .env_remove("MIRI_HEAP_GUARD")
        .env_remove("MIRI_LEAK_CHECK");
    if let Some(record) = record {
        command.env("MIRI_PROFILE_OUT", record);
    }
    command.output().unwrap()
}

/// A profile build carries the table of its functions the sampler places
/// addresses with; a release build of the same program does not.
#[test]
fn test_profile_build_carries_its_function_table_and_release_does_not() {
    let profile_dir = tempfile::tempdir().unwrap();
    let release_dir = tempfile::tempdir().unwrap();
    let profiled = build(profile_dir.path(), HOT_LOOP, &["--profile"]);
    let release = build(release_dir.path(), HOT_LOOP, &["--release"]);
    assert!(
        contains(&std::fs::read(&profiled).unwrap(), FUNCTION_TABLE),
        "the profile build has no function table"
    );
    assert!(
        !contains(&std::fs::read(&release).unwrap(), FUNCTION_TABLE),
        "the release build carries a function table"
    );
}

/// Run without `MIRI_PROFILE_OUT`, a profile build samples nothing: it runs
/// as a release build does, says nothing about profiling, and writes no file.
#[test]
fn test_profile_binary_without_the_variable_samples_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build(dir.path(), HOT_LOOP, &["--profile"]);
    let before: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    let output = run(&executable, None);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(!output.stdout.is_empty());
    let after: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(before.len(), after.len(), "the run wrote a file");
}

/// The record's stacks: each line's count and frames.
fn record_stacks(record: &str) -> Vec<(u64, Vec<String>)> {
    let mut lines = record.lines();
    assert_eq!(lines.next(), Some("miri-profile 1"), "{record}");
    lines
        .filter(|line| line.starts_with(|c: char| c.is_ascii_digit()))
        .map(|line| {
            let mut words = line.split(' ');
            let count = words.next().unwrap().parse().unwrap();
            (count, words.map(str::to_string).collect())
        })
        .collect()
}

/// The value of the header line `name` of a record.
fn record_header(record: &str, name: &str) -> u64 {
    record
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("no {name} in {record}"))
        .parse()
        .unwrap()
}

/// With `MIRI_PROFILE_OUT` set, a profile build writes a record whose
/// samples land in the program's own functions, named by their index in the
/// line map and never by a path on this machine.
#[test]
fn test_profile_binary_writes_its_record_where_the_variable_says() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build(dir.path(), HOT_LOOP, &["--profile"]);
    let record_path = dir.path().join("prog.profile");
    let output = run(&executable, Some(&record_path));
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let record = std::fs::read_to_string(&record_path).unwrap();
    assert_eq!(record_header(&record, "hz"), 1000);
    let samples = record_header(&record, "samples");
    assert!(samples > 20, "only {samples} samples:\n{record}");
    let stacks = record_stacks(&record);
    let in_program: u64 = stacks
        .iter()
        .filter(|(_, frames)| frames[0].starts_with('f'))
        .map(|(count, _)| count)
        .sum();
    assert!(
        in_program * 2 > samples,
        "most samples should land in the program's code:\n{record}"
    );
    assert!(
        stacks.iter().any(|(_, frames)| frames.len() > 1),
        "no sample walked to a caller:\n{record}"
    );
    assert!(!record.contains('/'), "the record names a path:\n{record}");
}

/// A program whose loop calls a library method on line 9. The method holds a
/// loop of its own, which keeps an optimized build from inlining it, so the
/// call stays a call a stack walk passes through.
const LIBRARY_LOOP: &str = r#"use system.io
use system.collections.list

fn mix_all(n int) int
    let values = List([3, 1, 4, 1, 5, 9, 2, 6])
    var acc = 0
    var i = 0
    while i < n
        acc = (acc + (values.sum() ?? 0) + i) % 1000003
        i += 1
    acc

fn main()
    println(f"{mix_all(40000000)}")
"#;

/// The line of `LIBRARY_LOOP` that calls into the library.
const LIBRARY_CALL_LINE: usize = 9;

/// The library file the method `LIBRARY_LOOP` calls is written in.
const LIBRARY_FILE: &str = "system/collections/foldable.mi";

/// A program whose loop calls, on line 8, a library function small enough
/// for an optimized build to inline into the loop.
const INLINED_LIBRARY_LOOP: &str = r#"use system.io
use system.math

fn mix_all(n int) u32
    var acc = 0 as u32
    var i = 0
    while i < n
        acc = acc ^ hash_u32(i as u32)
        i += 1
    acc

fn main()
    println(f"{mix_all(40000000)}")
"#;

/// Write `source` as `prog.mi` in `dir` and run `miri profile` on it with
/// `flags`, from `dir`.
fn profile(dir: &Path, source: &str, flags: &[&str]) -> std::process::Output {
    std::fs::write(dir.join("prog.mi"), source).unwrap();
    miri_cmd()
        .current_dir(dir)
        .arg("profile")
        .arg("prog.mi")
        .args(flags)
        .env_remove("MIRI_PROFILE_OUT")
        .env_remove("MIRI_PROFILE_HZ")
        .output()
        .unwrap()
}

/// The JSON envelope `miri profile --format json` printed.
fn profile_envelope(dir: &Path, source: &str, flags: &[&str]) -> serde_json::Value {
    let mut arguments = vec!["--format", "json"];
    arguments.extend_from_slice(flags);
    let output = profile(dir, source, &arguments);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "no envelope ({error}): {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// The `profile.lines` entry for `path:line`, if the report lists it.
fn reported_line<'e>(
    envelope: &'e serde_json::Value,
    path: &str,
    line: usize,
) -> Option<&'e serde_json::Value> {
    envelope["profile"]["lines"]
        .as_array()?
        .iter()
        .find(|entry| entry["path"] == path && entry["line"] == line)
}

/// The sum of the `selfShare` of the entries of `list` that `keep` admits.
fn self_share_of(list: &serde_json::Value, keep: impl Fn(&serde_json::Value) -> bool) -> f64 {
    list.as_array()
        .unwrap()
        .iter()
        .filter(|entry| keep(entry))
        .map(|entry| entry["selfShare"].as_f64().unwrap())
        .sum()
}

/// The line a program spends its time on is reported first, its source text
/// beside it, and the loop it sits in holds nearly every sample: an
/// instruction of the loop that carries no line of its own is charged to the
/// loop line before it, not left outside every line.
#[test]
fn test_profile_reports_the_hot_line_first() {
    let dir = tempfile::tempdir().unwrap();
    // Long enough for a few hundred samples from a sparse timer on a loaded
    // machine, in under two seconds.
    let source = HOT_LOOP.replace("spin(100000000)", "spin(400000000)");
    let envelope = profile_envelope(dir.path(), &source, &["--top", "100"]);
    assert_eq!(envelope["command"], "profile", "{envelope:#}");
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    let first = &envelope["profile"]["lines"][0];
    assert_eq!(first["path"], "prog.mi", "{envelope:#}");
    assert_eq!(first["line"], HOT_LINE, "{envelope:#}");
    let share = first["selfShare"].as_f64().unwrap();
    assert!(first["inclusiveShare"].as_f64().unwrap() >= share);
    let in_loop = self_share_of(&envelope["profile"]["lines"], |entry| {
        entry["path"] == "prog.mi" && LOOP_LINES.iter().any(|&line| entry["line"] == line)
    });
    assert!(in_loop > 0.9, "the loop holds only {in_loop}: {envelope:#}");
    let outside = self_share_of(&envelope["profile"]["runtime"], |_| true);
    assert!(
        outside < 0.1,
        "{outside} falls outside every line: {envelope:#}"
    );
    assert_eq!(
        first["text"], "        acc = (acc * 31 + i) % 1000003",
        "{envelope:#}"
    );
    assert!(envelope["profile"]["samples"].as_u64().unwrap() > 20);
    assert!(envelope["stdoutTail"]
        .as_str()
        .unwrap()
        .trim()
        .parse::<i64>()
        .is_ok());
    let typed: miri::diagnostics::json::DiagnosticsEnvelope =
        serde_json::from_value(envelope.clone())
            .expect("the envelope reads back as the documented type, with no unknown field");
    assert_eq!(typed.schema_version, 1);
    assert!(typed.profile.is_some());
}

/// Profile `source` and check the time spent in `library` is charged to
/// the library's own lines as self time, and to `call_line` of the program
/// inclusively.
fn assert_library_time_reaches_its_caller(source: &str, library: &str, call_line: usize) {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(dir.path(), source, &["--top", "100"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    let lines = envelope["profile"]["lines"].as_array().unwrap();
    let library_self: u64 = lines
        .iter()
        .filter(|entry| entry["path"] == library)
        .map(|entry| entry["selfSamples"].as_u64().unwrap())
        .sum();
    assert!(library_self > 0, "no time in the library: {envelope:#}");
    let caller = reported_line(&envelope, "prog.mi", call_line)
        .unwrap_or_else(|| panic!("the calling line is not reported: {envelope:#}"));
    let inclusive = caller["inclusiveSamples"].as_u64().unwrap();
    assert!(
        inclusive >= library_self,
        "the caller's inclusive time {inclusive} misses the library's {library_self}"
    );
}

/// Time spent in a library function inlined into its caller is charged to
/// the library's own line as self time, and to the line calling it
/// inclusively, as if the call had stayed a call.
#[test]
fn test_profile_charges_inlined_library_time_to_the_library_and_its_caller() {
    assert_library_time_reaches_its_caller(INLINED_LIBRARY_LOOP, "system/math.mi", 8);
}

/// Time spent in a library method the caller calls is charged to the
/// library's own line as self time, and to the line calling it inclusively.
#[test]
fn test_profile_charges_library_time_to_the_library_and_its_caller() {
    assert_library_time_reaches_its_caller(LIBRARY_LOOP, LIBRARY_FILE, LIBRARY_CALL_LINE);
}

/// In the pretty form the program's own output comes through unchanged and
/// the report follows it on stderr.
#[test]
fn test_profile_passes_the_programs_output_through_then_reports_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let output = profile(dir.path(), HOT_LOOP, &[]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().parse::<i64>().is_ok(),
        "stdout is not the program's: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("prog.mi:{HOT_LINE}")), "{stderr}");
    assert!(
        stderr.contains("acc = (acc * 31 + i) % 1000003"),
        "{stderr}"
    );
}

/// `--top` bounds the lines reported and `--hz` sets the rate sampled at.
#[test]
fn test_profile_top_and_hz_shape_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(dir.path(), HOT_LOOP, &["--top", "1", "--hz", "500"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    assert_eq!(envelope["profile"]["lines"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["profile"]["hz"], 500);
}

/// Nothing in the report names a path on the machine that ran it.
#[test]
fn test_profile_report_names_no_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(dir.path(), LIBRARY_LOOP, &["--top", "100"]);
    let text = envelope["profile"].to_string();
    let home = std::env::var("HOME").unwrap_or_default();
    let directory = dir.path().canonicalize().unwrap();
    for absolute in [
        home.as_str(),
        &directory.display().to_string(),
        "/tmp",
        "/var",
    ] {
        assert!(!absolute.is_empty() && !text.contains(absolute), "{text}");
    }
}

/// A program over before the first sample leaves nothing to report, and the
/// command says so rather than printing an empty table.
#[test]
fn test_profile_of_a_program_too_short_to_sample_reports_no_samples() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(
        dir.path(),
        "use system.io\n\nfn main()\n    println(\"hi\")\n",
        &[],
    );
    assert_eq!(envelope["ok"], false, "{envelope:#}");
    assert_eq!(
        envelope["diagnostics"][0]["code"], "MER_BLD_028",
        "{envelope:#}"
    );
    assert_eq!(envelope["stdoutTail"], "hi\n", "{envelope:#}");
}

/// A program that dies while profiled is reported as failed, with the
/// reason it died after.
#[test]
fn test_profile_of_a_program_that_traps_reports_it_failed() {
    let dir = tempfile::tempdir().unwrap();
    let source = "use system.io\n\nfn main()\n    let xs = [1, 2, 3]\n    var i = 3\n    println(f\"{xs[i]}\")\n";
    let envelope = profile_envelope(dir.path(), source, &[]);
    assert_eq!(envelope["ok"], false, "{envelope:#}");
    assert_eq!(
        envelope["diagnostics"][0]["code"], "MER_BLD_029",
        "{envelope:#}"
    );
    assert_eq!(envelope["exitCode"], 1, "{envelope:#}");
    assert!(envelope.get("profile").is_none(), "{envelope:#}");
}

/// A program that runs long enough to be sampled and then traps is still
/// reported as failed, and the samples it took up to the trap come with it.
#[test]
fn test_profile_of_a_program_that_traps_late_keeps_the_samples_it_took() {
    let dir = tempfile::tempdir().unwrap();
    let source = HOT_LOOP.replace(
        "    println(f\"{spin(100000000)}\")\n",
        "    let xs = [1, 2, 3]\n    var i = spin(100000000) * 0 + 3\n    println(f\"{xs[i]}\")\n",
    );
    let envelope = profile_envelope(dir.path(), &source, &[]);
    assert_eq!(envelope["ok"], false, "{envelope:#}");
    assert_eq!(
        envelope["diagnostics"][0]["code"], "MER_BLD_029",
        "{envelope:#}"
    );
    assert_eq!(
        envelope["diagnostics"][1]["code"], "MER_RT_011",
        "{envelope:#}"
    );
    assert_eq!(
        envelope["profile"]["lines"][0]["line"], HOT_LINE,
        "{envelope:#}"
    );
}

/// A program that does not compile is reported as `run` reports it.
#[test]
fn test_profile_of_a_program_that_does_not_compile_reports_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(dir.path(), "fn main()\n    let x int = \"text\"\n", &[]);
    assert_eq!(envelope["ok"], false, "{envelope:#}");
    assert!(
        envelope["diagnostics"][0]["code"]
            .as_str()
            .is_some_and(|code| code.starts_with("MER_TYP")),
        "{envelope:#}"
    );
}
