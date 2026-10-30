// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A program that spends most of its time on line 9, the loop body's second
/// statement: its modulus is known only at run time, so the line carries a
/// hardware divide and the checks for a zero divisor and an overflowing
/// quotient, rather than a multiply by a constant the optimizer could make
/// of a constant modulus.
const HOT_LOOP: &str = r#"use system.io

fn spin(n int) int
    let modulus = n / 100 + 3
    var acc = 0
    var i = 0
    while i < n
        i += 1
        acc = (acc * 31 + i) % modulus
    acc

fn main()
    println(f"{spin(100000000)}")
"#;

/// The line of `HOT_LOOP` its time goes to.
const HOT_LINE: usize = 9;

/// The text of `HOT_LINE`.
const HOT_TEXT: &str = "        acc = (acc * 31 + i) % modulus";

/// The line of `HOT_LOOP` holding `spin`'s signature.
fn signature_line_of_spin() -> usize {
    HOT_LOOP
        .lines()
        .position(|line| line.starts_with("fn spin("))
        .map(|index| index + 1)
        .unwrap_or_else(|| panic!("no signature of spin in HOT_LOOP"))
}

/// The lines of `HOT_LOOP`'s loop: `var i`, the `while`, and its body.
const LOOP_LINES: [usize; 4] = [6, 7, 8, 9];

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
/// for an optimized build to inline into the loop. Inlined, an iteration takes
/// about a nanosecond, so the loop runs for about half a second, long enough
/// for a few hundred samples.
pub(crate) const INLINED_LIBRARY_LOOP: &str = r#"use system.io
use system.math

fn mix_all(n int) u32
    var acc = 0 as u32
    var i = 0
    while i < n
        acc = acc ^ hash_u32(i as u32)
        i += 1
    acc

fn main()
    println(f"{mix_all(400000000)}")
"#;

/// The line of `INLINED_LIBRARY_LOOP` calling the inlined library function.
pub(crate) const INLINED_CALL_LINE: usize = 8;

/// The lines of `INLINED_LIBRARY_LOOP`'s loop: `var i`, the `while`, and its
/// body.
const INLINED_LOOP_LINES: [usize; 4] = [6, 7, 8, 9];

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

/// The loop a program spends its time in holds nearly every sample, the hot
/// line among them with a substantial share of its own and its source text
/// beside it, and none is charged to the function's signature: an
/// instruction of the loop that carries no line of its own is charged to a
/// line of the loop, not left outside every line nor given to whatever code
/// is laid out before its block.
///
/// That the hot line comes first is not asserted. A timer sample lands on
/// an instruction after the one it interrupts, and how far it skids depends
/// on the processor: on x86-64 the samples of the divide land on the loop's
/// head and increment about as often as on the divide's own line.
#[test]
fn test_profile_charges_the_loop_to_its_lines_and_the_hot_line_carries_its_share() {
    let dir = tempfile::tempdir().unwrap();
    // Twice the fixture's iterations: each pays for a hardware divide, so
    // the run takes about a second, long enough for a few hundred samples
    // from a sparse timer on a loaded machine.
    let source = HOT_LOOP.replace("spin(100000000)", "spin(200000000)");
    let envelope = profile_envelope(dir.path(), &source, &["--top", "100"]);
    assert_eq!(envelope["command"], "profile", "{envelope:#}");
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    let hot = reported_line(&envelope, "prog.mi", HOT_LINE)
        .unwrap_or_else(|| panic!("the hot line is not reported: {envelope:#}"));
    let share = hot["selfShare"].as_f64().unwrap();
    assert!(
        share >= 0.2,
        "the hot line holds only {share}: {envelope:#}"
    );
    assert!(hot["inclusiveShare"].as_f64().unwrap() >= share);
    assert_eq!(hot["text"], HOT_TEXT, "{envelope:#}");
    let signature = signature_line_of_spin();
    let on_signature = reported_line(&envelope, "prog.mi", signature)
        .map_or(0, |entry| entry["selfSamples"].as_u64().unwrap());
    assert_eq!(
        on_signature, 0,
        "samples are charged to `spin`'s signature on line {signature}: {envelope:#}"
    );
    let in_loop = self_share_of(&envelope["profile"]["lines"], |entry| {
        entry["path"] == "prog.mi" && LOOP_LINES.iter().any(|&line| entry["line"] == line)
    });
    assert!(in_loop > 0.9, "the loop holds only {in_loop}: {envelope:#}");
    let outside = self_share_of(&envelope["profile"]["runtime"], |_| true);
    assert!(
        outside < 0.1,
        "{outside} falls outside every line: {envelope:#}"
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

/// A profile of a loop with a library function inlined into it charges
/// whatever time it gives the library's lines to the calling line as well,
/// and keeps nearly all of it on the loop's lines.
///
/// Library self time is not asserted. A timer sample lands on the
/// instruction after a branch, so the time of a run of straight-line code is
/// charged to the code a taken branch reaches next, here the loop's head and
/// the counter's increment; no instruction of the inlined function follows a
/// taken branch, so the library often gets no sample at all. That the
/// inlined code is charged to the library through line 8 is proven without
/// sampling, by the line map
/// (`test_profile_line_map_charges_inlined_library_code_through_its_call`).
#[test]
fn test_profile_charges_inlined_library_samples_within_the_calling_loop() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(dir.path(), INLINED_LIBRARY_LOOP, &["--top", "100"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    let lines = &envelope["profile"]["lines"];
    let library_self: u64 = (lines.as_array().unwrap().iter())
        .filter(|entry| entry["path"] == "system/math.mi")
        .map(|entry| entry["selfSamples"].as_u64().unwrap())
        .sum();
    let inclusive = reported_line(&envelope, "prog.mi", INLINED_CALL_LINE)
        .map_or(0, |caller| caller["inclusiveSamples"].as_u64().unwrap());
    assert!(
        inclusive >= library_self,
        "the calling line's inclusive {inclusive} misses the library's {library_self}: \
         {envelope:#}"
    );
    let in_loop = self_share_of(lines, |entry| {
        entry["path"] == "system/math.mi"
            || (entry["path"] == "prog.mi"
                && INLINED_LOOP_LINES.iter().any(|&line| entry["line"] == line))
    });
    assert!(
        in_loop > 0.9,
        "only {in_loop} of the time is in the loop: {envelope:#}"
    );
}

/// Time spent in a library method the caller calls is charged to the
/// library's own line as self time, and to the line calling it inclusively.
#[test]
fn test_profile_charges_library_time_to_the_library_and_its_caller() {
    assert_library_time_reaches_its_caller(LIBRARY_LOOP, LIBRARY_FILE, LIBRARY_CALL_LINE);
}

/// A program that spends most of its time allocating and releasing class
/// instances, which happens in the runtime rather than on any source line.
const ALLOCATING_LOOP: &str = r#"use system.io

class Node
    var left Node?
    var right Node?

    fn init(left Node?, right Node?)
        self.left = left
        self.right = right

fn bottom_up(depth int) Node
    if depth == 0
        return Node(None, None)
    Node(bottom_up(depth - 1), bottom_up(depth - 1))

fn check(node Node) int
    match node.left
        Some(l)
            match node.right
                Some(r): return 1 + check(l) + check(r)
                None: return 1
        None: return 1

fn main()
    var total = 0
    var i = 0
    while i < 128
        total += check(bottom_up(16))
        i += 1
    println(f"{total}")
"#;

/// Time spent in the runtime's allocator is charged to its functions by
/// name, and next to none is left unnamed: on Linux the loader names only
/// the symbols an executable exports.
#[test]
fn test_profile_names_the_runtime_functions_a_program_spends_time_in() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = profile_envelope(dir.path(), ALLOCATING_LOOP, &["--top", "100"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    let runtime = &envelope["profile"]["runtime"];
    let is_named = |entry: &serde_json::Value| {
        entry["symbol"]
            .as_str()
            .is_some_and(|symbol| symbol.contains("miri_rt_block_"))
    };
    let named = self_share_of(runtime, is_named);
    assert!(
        named > 0.0,
        "no time is charged to the block allocator: {envelope:#}"
    );
    let unknown = self_share_of(runtime, |entry| entry["symbol"] == "(unknown)");
    assert!(
        unknown < 0.1,
        "{unknown} of the time is in functions left unnamed: {envelope:#}"
    );
}

/// The line of `ALLOCATING_LOOP` that releases each tree it builds.
const RELEASING_LINE: usize = 28;

/// The code that releases a class instance runs in a function the compiler
/// generated, which has no line of its own: its time is charged to the line
/// the release happens on, not listed beside the runtime's functions.
#[test]
fn test_profile_charges_time_releasing_instances_to_the_releasing_line() {
    let dir = tempfile::tempdir().unwrap();
    let source = ALLOCATING_LOOP.replace("i < 128", "i < 256");
    let envelope = profile_envelope(dir.path(), &source, &["--top", "100"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");
    let helpers = self_share_of(&envelope["profile"]["runtime"], |entry| {
        entry["symbol"]
            .as_str()
            .is_some_and(|symbol| symbol.contains("$drop"))
    });
    assert_eq!(
        helpers, 0.0,
        "a drop helper is charged by name: {envelope:#}"
    );
    let releasing = reported_line(&envelope, "prog.mi", RELEASING_LINE)
        .and_then(|line| line["selfShare"].as_f64());
    assert!(
        releasing.is_some_and(|share| share > 0.0),
        "no time is charged to the releasing line: {envelope:#}"
    );
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
    assert!(stderr.contains(HOT_TEXT.trim_start()), "{stderr}");
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
    let hot_samples = reported_line(&envelope, "prog.mi", HOT_LINE)
        .map_or(0, |entry| entry["selfSamples"].as_u64().unwrap());
    assert!(
        hot_samples > 0,
        "the hot line took no sample before the trap: {envelope:#}"
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
