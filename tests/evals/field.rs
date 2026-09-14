// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Structural gate over the live-model benchmark under `evals/field/`.
//!
//! The replay corpus beside it measures a recorded transcript against the real
//! compiler. This one measures a live agent, so nothing about a run can be
//! asserted from here — only that the instrument it runs on is intact and says
//! the same thing to every arm.
//!
//! That is the whole point of the gate. A benchmark whose briefs drift between
//! arms, whose planted faults live in prose, or whose expected outputs were
//! typed by hand rather than produced by a reference, measures the instrument
//! instead of the subject, and it does so silently.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One job in the benchmark, in the order the report lists them.
struct Job {
    /// Directory under `evals/field/jobs/`.
    id: &'static str,
    /// Whether the subject starts from a committed source rather than nothing.
    seeded: bool,
    /// How many hidden-test cases the job must carry.
    ///
    /// A job with one case measures whether the agent compiled something, not
    /// whether it read the contract.
    min_cases: usize,
}

/// The five jobs of a round.
const JOBS: [Job; 5] = [
    Job {
        id: "01-word-frequency",
        seeded: false,
        min_cases: 4,
    },
    Job {
        id: "02-ledger-repair",
        seeded: true,
        min_cases: 4,
    },
    Job {
        id: "03-tracker-extension",
        seeded: true,
        min_cases: 4,
    },
    Job {
        id: "04-data-edges",
        seeded: false,
        min_cases: 5,
    },
    Job {
        id: "05-gpu-heat",
        seeded: false,
        min_cases: 3,
    },
];

/// The five arms every job is run under.
const ARMS: [&str; 5] = ["miri-pack", "miri-bare", "python", "rust", "typescript"];

/// The languages a seeded job commits a starting source for.
const SEED_LANGUAGES: [&str; 4] = ["miri", "python", "rust", "typescript"];

/// Words that would tell a subject which language it is being measured in.
///
/// A brief is rendered per arm; the source text must carry the language only
/// through its placeholders, or the arms are no longer reading the same job.
const LANGUAGE_WORDS: [&str; 8] = [
    "python",
    "rust",
    "typescript",
    "miri",
    "deno",
    "cargo",
    "pytest",
    "wgpu",
];

#[derive(Deserialize)]
struct ArmsFile {
    arm: Vec<Arm>,
}

#[derive(Deserialize)]
struct Arm {
    id: String,
    language: String,
    toolchain: String,
    entry_point: String,
    run: Vec<String>,
}

#[derive(Deserialize)]
struct JobFile {
    id: String,
    title: String,
    turn_cap: u32,
    time_cap_seconds: u32,
}

fn field_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("evals")
        .join("field")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e))
}

/// The `.in` files of a job, sorted, with their matching `.out` beside them.
fn cases(job: &str) -> Vec<(PathBuf, PathBuf)> {
    let dir = field_dir().join("jobs").join(job).join("cases");
    let mut inputs: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "in"))
        .collect();
    inputs.sort();
    inputs
        .into_iter()
        .map(|input| (input.with_extension("out"), input))
        .collect()
}

/// Whether an external program is on `PATH`.
fn available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

#[test]
fn test_every_job_has_a_brief_and_paired_cases() {
    for job in &JOBS {
        let dir = field_dir().join("jobs").join(job.id);
        assert!(dir.join("BRIEF.md").is_file(), "{} has no BRIEF.md", job.id);
        assert!(dir.join("job.toml").is_file(), "{} has no job.toml", job.id);

        let pairs = cases(job.id);
        assert!(
            pairs.len() >= job.min_cases,
            "{} carries {} hidden-test cases, fewer than the {} it must",
            job.id,
            pairs.len(),
            job.min_cases
        );
        for (expected, input) in pairs {
            assert!(
                expected.is_file(),
                "{} has no expected output beside it",
                input.display()
            );
        }
    }
}

#[test]
fn test_job_metadata_names_the_job_and_caps_it() {
    for job in &JOBS {
        let path = field_dir().join("jobs").join(job.id).join("job.toml");
        let parsed: JobFile = toml::from_str(&read(&path))
            .unwrap_or_else(|e| panic!("cannot parse {}: {}", path.display(), e));
        assert_eq!(
            parsed.id,
            job.id,
            "{} declares a different id",
            path.display()
        );
        assert!(!parsed.title.is_empty(), "{} has no title", path.display());
        assert!(parsed.turn_cap > 0, "{} caps turns at zero", path.display());
        assert!(
            parsed.time_cap_seconds > 0,
            "{} caps time at zero",
            path.display()
        );
    }
}

#[test]
fn test_briefs_are_language_neutral() {
    for job in &JOBS {
        let path = field_dir().join("jobs").join(job.id).join("BRIEF.md");
        let brief = read(&path);
        for placeholder in ["{{LANGUAGE}}", "{{TOOLCHAIN}}", "{{ENTRY_POINT}}"] {
            assert!(
                brief.contains(placeholder),
                "{} never substitutes {}",
                path.display(),
                placeholder
            );
        }
        let lowered = brief.to_lowercase();
        for word in LANGUAGE_WORDS {
            assert!(
                !lowered.contains(word),
                "{} names '{}', so the arms do not read the same brief",
                path.display(),
                word
            );
        }
    }
}

#[test]
fn test_seeded_jobs_ship_a_starting_source_for_every_language() {
    for job in JOBS.iter().filter(|job| job.seeded) {
        for language in SEED_LANGUAGES {
            let dir = field_dir()
                .join("jobs")
                .join(job.id)
                .join("seeds")
                .join(language);
            assert!(dir.is_dir(), "{} has no {} seed", job.id, language);
            let files = fs::read_dir(&dir)
                .map(|entries| entries.count())
                .unwrap_or(0);
            assert!(files > 0, "{} is empty", dir.display());
        }
    }
}

#[test]
fn test_jobs_that_start_from_nothing_carry_no_seed() {
    for job in JOBS.iter().filter(|job| !job.seeded) {
        let dir = field_dir().join("jobs").join(job.id).join("seeds");
        assert!(
            !dir.exists(),
            "{} starts from nothing but carries {}",
            job.id,
            dir.display()
        );
    }
}

#[test]
fn test_the_six_planted_faults_are_mirrored_across_every_port() {
    let job = field_dir().join("jobs").join("02-ledger-repair");
    let faults = read(&job.join("FAULTS.md"));
    let anchors = fault_anchors(&faults);
    assert_eq!(
        anchors.len(),
        6,
        "the repair job declares {} faults, not six",
        anchors.len()
    );

    for language in SEED_LANGUAGES {
        let source = concatenated_seed(&job.join("seeds").join(language));
        for (fault, anchor) in &anchors {
            assert!(
                source.contains(anchor),
                "{} is not planted in the {} port: no line holds `{}`",
                fault,
                language,
                anchor
            );
        }
    }
}

/// Read the `F<n> | <anchor>` rows out of the fault table.
///
/// The anchor is the text the faulted line carries in every port. It is what
/// makes "the same six faults at the same points" checkable rather than
/// asserted: a port that drops a fault no longer holds its anchor.
fn fault_anchors(table: &str) -> Vec<(String, String)> {
    table
        .lines()
        .filter_map(|line| {
            let mut columns = line.trim_matches('|').split('|').map(str::trim);
            let id = columns.next()?.to_string();
            if !is_fault_id(&id) {
                return None;
            }
            let anchor = columns.next_back()?.trim_matches('`').to_string();
            Some((id, anchor))
        })
        .collect()
}

/// Whether a table cell names a fault — `F` and a digit, not the header word.
fn is_fault_id(cell: &str) -> bool {
    let mut characters = cell.chars();
    characters.next() == Some('F') && characters.next().is_some_and(|c| c.is_ascii_digit())
}

/// Every seed file of one port, joined, so an anchor may sit in any of them.
fn concatenated_seed(dir: &Path) -> String {
    let mut text = String::new();
    collect_sources(dir, &mut text);
    text
}

fn collect_sources(dir: &Path, text: &mut String) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e));
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, text);
        } else if let Ok(contents) = fs::read_to_string(&path) {
            text.push_str(&contents);
        }
    }
}

#[test]
fn test_arms_are_the_five_the_benchmark_compares() {
    let path = field_dir().join("arms.toml");
    let parsed: ArmsFile = toml::from_str(&read(&path))
        .unwrap_or_else(|e| panic!("cannot parse {}: {}", path.display(), e));

    let declared: BTreeSet<&str> = parsed.arm.iter().map(|arm| arm.id.as_str()).collect();
    let expected: BTreeSet<&str> = ARMS.into_iter().collect();
    assert_eq!(
        declared,
        expected,
        "{} declares a different set of arms",
        path.display()
    );

    for arm in &parsed.arm {
        assert!(!arm.language.is_empty(), "arm {} names no language", arm.id);
        assert!(
            !arm.toolchain.is_empty(),
            "arm {} names no toolchain",
            arm.id
        );
        assert!(
            !arm.entry_point.is_empty(),
            "arm {} names no entry point",
            arm.id
        );
        assert!(!arm.run.is_empty(), "arm {} has no run command", arm.id);
    }
}

#[test]
fn test_readme_states_the_two_measurement_rules() {
    let readme = read(&field_dir().join("README.md"));
    assert!(
        readme.contains("never asked for a per-tool verdict"),
        "the README does not state the no-per-tool-verdicts rule"
    );
    assert!(
        readme.contains("counted separately"),
        "the README does not state that defect-cornering is counted separately"
    );
    assert!(
        readme.contains("evals/") && readme.contains("replay"),
        "the README does not separate the replay corpus from this one"
    );
}

#[test]
fn test_claims_are_pre_registered_with_their_thresholds() {
    let claims = read(&field_dir().join("CLAIMS.md"));
    for id in ["C1", "C2", "C3", "C4"] {
        assert!(claims.contains(id), "CLAIMS.md does not carry {}", id);
    }
    assert!(
        claims.contains("1.25"),
        "CLAIMS.md drops C1's cost-parity threshold"
    );
    assert!(
        claims.contains("miri-bare"),
        "CLAIMS.md drops the arm C4 compares against"
    );
}

#[test]
fn test_baseline_carries_the_third_field_test_table() {
    let baseline = read(&field_dir().join("baseline.md"));
    for number in ["53+", "20", "9", "6", "50", "13", "26"] {
        assert!(
            baseline.contains(number),
            "baseline.md drops the recorded number {}",
            number
        );
    }
    assert!(
        baseline.contains("not comparable"),
        "baseline.md does not say its numbers are not comparable to the revised arms"
    );
    assert!(
        baseline.contains("per-tool verdict"),
        "baseline.md does not record the overhead its tooled column carried"
    );
}

#[test]
fn test_the_runner_and_the_folder_are_valid_programs() {
    let dir = field_dir();
    for script in ["bench.py", "report.py"] {
        assert!(dir.join(script).is_file(), "{} is missing", script);
    }
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the harness scripts were not compiled");
        return;
    }
    for script in ["bench.py", "report.py"] {
        let output = Command::new("python3")
            .arg("-m")
            .arg("py_compile")
            .arg(dir.join(script))
            .output()
            .expect("cannot run python3");
        assert!(
            output.status.success(),
            "{} does not compile:\n{}",
            script,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_expected_outputs_come_from_the_reference_solution() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the reference was not run");
        return;
    }
    for job in &JOBS {
        let reference = field_dir()
            .join("jobs")
            .join(job.id)
            .join("reference")
            .join("solution.py");
        assert!(reference.is_file(), "{} has no reference solution", job.id);
        for (expected, input) in cases(job.id) {
            let produced = run_reference(&reference, &input);
            assert_eq!(
                produced,
                read(&expected),
                "{} does not match what the reference produces for {}",
                expected.display(),
                input.display()
            );
        }
    }
}

/// Run a reference solution over one case's input and return its stdout.
///
/// A program under test names its input in its first argument rather than
/// reading standard input, because one of the five languages cannot read
/// standard input at all. The reference is run the same way.
fn run_reference(reference: &Path, input: &Path) -> String {
    let output = Command::new("python3")
        .arg(reference)
        .arg(input)
        .output()
        .expect("cannot run python3");
    assert!(
        output.status.success(),
        "the reference failed on {}:\n{}",
        input.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_the_planted_faults_are_real() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the faulted port was not run");
        return;
    }
    let job = field_dir().join("jobs").join("02-ledger-repair");
    let seed = job.join("seeds").join("python").join("main.py");
    let failures = cases("02-ledger-repair")
        .into_iter()
        .filter(|(expected, input)| run_seed(&seed, input) != read(expected))
        .count();
    assert!(
        failures > 0,
        "the faulted port passes every hidden test, so the six planted faults repair nothing"
    );
}

/// Run a faulted seed over one case. A seed may crash; that is a fault too.
fn run_seed(seed: &Path, input: &Path) -> String {
    let output = Command::new("python3")
        .arg(seed)
        .arg(input)
        .output()
        .expect("cannot run python3");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The jobs the CPU claims are judged over. The GPU job has a claim of its own.
const CPU_JOBS: [&str; 4] = [
    "01-word-frequency",
    "02-ledger-repair",
    "03-tracker-extension",
    "04-data-edges",
];

/// The languages a Miri arm is compared against.
const BASELINE_ARMS: [&str; 3] = ["python", "rust", "typescript"];

/// What one synthetic run cost and whether it reached green.
#[derive(Clone, Copy)]
struct SyntheticRun {
    tokens: u32,
    seconds: u32,
    invocations: u32,
    green: bool,
}

impl SyntheticRun {
    fn green(tokens: u32, seconds: u32, invocations: u32) -> Self {
        SyntheticRun {
            tokens,
            seconds,
            invocations,
            green: true,
        }
    }
}

/// A cell of a synthetic round, written where the folder will read it.
fn write_synthetic_record(root: &Path, job: &str, arm: &str, run: SyntheticRun) {
    let directory = root
        .join("synthetic")
        .join(job)
        .join(arm)
        .join("claude-sonnet");
    fs::create_dir_all(&directory).expect("cannot create a synthetic round");
    let passed = if run.green { 6 } else { 3 };
    let record = format!(
        r#"{{"schemaVersion":1,"round":"synthetic","job":"{job}","arm":"{arm}","run":1,
            "model":"claude-sonnet","modelId":"x","harness":{{"name":"claude","version":"1"}},
            "compilerCommit":"0","compilerVersion":"0","packInstalled":false,
            "caps":{{"turns":1,"timeSeconds":1}},"startedAt":"now","wallClockSeconds":{seconds}.0,
            "tokens":{{"in":{half},"out":{half}}},"turns":1,"toolInvocations":{invocations},
            "toolchainInvocations":1,"outcome":"finished",
            "hiddenTests":{{"passed":{passed},"total":6,"failures":[]}},
            "silentWrongAnswer":false,"workspace":"/tmp"}}"#,
        job = job,
        arm = arm,
        half = run.tokens / 2,
        seconds = run.seconds,
        invocations = run.invocations,
        passed = passed,
    );
    fs::write(directory.join("1.json"), record).expect("cannot write a synthetic record");
}

/// A synthetic round over every CPU job: the pack at one cost, every baseline
/// at another.
fn write_pack_against_baselines(root: &Path, pack: SyntheticRun, baseline: SyntheticRun) {
    for job in CPU_JOBS {
        write_synthetic_record(root, job, "miri-pack", pack);
        for arm in BASELINE_ARMS {
            write_synthetic_record(root, job, arm, baseline);
        }
    }
}

/// Fold a synthetic round and report whether each claim held.
fn fold(root: &Path) -> String {
    let output = Command::new("python3")
        .arg(field_dir().join("report.py"))
        .args(["--round", "synthetic", "--runs-root"])
        .arg(root)
        .arg("--out")
        .arg(root.join("summary.json"))
        .output()
        .expect("cannot run python3");
    assert!(
        output.status.success(),
        "the folder failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    read(&root.join("summary.json"))
}

/// Whether a folded summary reports one claim as held.
///
/// Read by finding the claim and then its verdict, rather than by matching the
/// formatter's indentation: a gate that a reflow can break is a gate nobody
/// trusts the next time it goes red.
fn claim_held(summary: &str, claim: &str) -> bool {
    let Some(section) = summary.split_once(&format!("\"{}\": {{", claim)) else {
        panic!("the summary carries no {}:\n{}", claim, summary);
    };
    let Some((_, verdict)) = section.1.split_once("\"held\":") else {
        panic!("{} carries no verdict:\n{}", claim, summary);
    };
    verdict.trim_start().starts_with("true")
}

#[test]
fn test_the_claim_judge_reads_the_data_it_is_given() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-claim-judge");
    let _ = fs::remove_dir_all(&scratch);

    // The pack is cheaper than the bare arm and finishes where it does not.
    let job = "01-word-frequency";
    write_synthetic_record(&scratch, job, "miri-pack", SyntheticRun::green(1000, 1, 1));
    let unfinished = SyntheticRun {
        green: false,
        ..SyntheticRun::green(3000, 1, 1)
    };
    write_synthetic_record(&scratch, job, "miri-bare", unfinished);
    let held = fold(&scratch);
    assert!(
        claim_held(&held, "C4"),
        "the judge does not report C4 as held on data that satisfies it:\n{}",
        held
    );

    // The same shape with the pack dearer than the bare arm must fail C4, or
    // the verdict is a constant and the article would publish on a constant.
    let _ = fs::remove_dir_all(&scratch);
    write_synthetic_record(&scratch, job, "miri-pack", SyntheticRun::green(4000, 1, 1));
    write_synthetic_record(&scratch, job, "miri-bare", SyntheticRun::green(1000, 1, 1));
    let refused = fold(&scratch);
    assert!(
        !claim_held(&refused, "C4"),
        "the judge reports C4 as held on data that refutes it:\n{}",
        refused
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_the_pack_must_beat_every_baseline_on_cost_and_on_speed() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-lead-claims");
    let _ = fs::remove_dir_all(&scratch);

    // Cheaper and faster than every language in every CPU job.
    write_pack_against_baselines(
        &scratch,
        SyntheticRun::green(1000, 100, 1),
        SyntheticRun::green(1100, 110, 1),
    );
    let leads = fold(&scratch);
    for claim in ["C1", "C5"] {
        assert!(
            claim_held(&leads, claim),
            "the judge does not report {} as held when the pack leads every baseline:\n{}",
            claim,
            leads
        );
    }

    // Within a tenth of every baseline, but behind it. Parity is reported as
    // parity and is not a win, or the article would call a tie a lead.
    let _ = fs::remove_dir_all(&scratch);
    write_pack_against_baselines(
        &scratch,
        SyntheticRun::green(1100, 110, 1),
        SyntheticRun::green(1000, 100, 1),
    );
    let tied = fold(&scratch);
    for claim in ["C1", "C5"] {
        assert!(
            !claim_held(&tied, claim),
            "the judge reports {} as held when the pack only matches the baselines:\n{}",
            claim,
            tied
        );
    }
    assert!(
        tied.contains("\"standing\": \"parity\""),
        "the judge does not report a result inside the parity band as parity:\n{}",
        tied
    );

    // A baseline that was never run is not a baseline the pack beat.
    let _ = fs::remove_dir_all(&scratch);
    for job in CPU_JOBS {
        write_synthetic_record(&scratch, job, "miri-pack", SyntheticRun::green(1, 1, 1));
    }
    write_synthetic_record(
        &scratch,
        "05-gpu-heat",
        "miri-pack",
        SyntheticRun::green(1, 1, 1),
    );
    let alone = fold(&scratch);
    for claim in ["C1", "C3", "C5"] {
        assert!(
            !claim_held(&alone, claim),
            "the judge reports {} as held against baselines that were never run:\n{}",
            claim,
            alone
        );
    }
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_the_exit_criterion_compares_the_pack_loop_against_the_bare_loop() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-pack-loop");
    let _ = fs::remove_dir_all(&scratch);
    let job = "01-word-frequency";

    write_synthetic_record(&scratch, job, "miri-pack", SyntheticRun::green(1, 1, 10));
    write_synthetic_record(&scratch, job, "miri-bare", SyntheticRun::green(1, 1, 12));
    let held = fold(&scratch);
    assert!(
        claim_held(&held, "packLoop"),
        "the judge does not report the pack loop as no worse than the bare loop:\n{}",
        held
    );

    let _ = fs::remove_dir_all(&scratch);
    write_synthetic_record(&scratch, job, "miri-pack", SyntheticRun::green(1, 1, 14));
    write_synthetic_record(&scratch, job, "miri-bare", SyntheticRun::green(1, 1, 12));
    let refused = fold(&scratch);
    assert!(
        !claim_held(&refused, "packLoop"),
        "the judge reports the pack loop as held when it costs more invocations:\n{}",
        refused
    );
    let _ = fs::remove_dir_all(&scratch);
}

/// A condition a verdict is computed from, as a document states it: the record
/// fields it reads and the key `report.py` publishes its verdict under.
struct MeasuredCondition {
    fields: Vec<String>,
    verdict: String,
}

/// Every `Measured from `a`, `b`; judged as `X`.` sentence in a document.
///
/// Whitespace is collapsed first, so a sentence the formatter wrapped across
/// lines reads the same as one that fits on a line.
fn measured_conditions(document: &str) -> Vec<MeasuredCondition> {
    let flat = document.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.split("Measured from ")
        .skip(1)
        .map(|sentence| {
            let sentence = sentence.split(". ").next().unwrap_or(sentence);
            let (fields, verdict) = sentence
                .split_once("judged as")
                .unwrap_or_else(|| panic!("a measured condition names no verdict: {}", sentence));
            MeasuredCondition {
                fields: backticked(fields),
                verdict: backticked(verdict).into_iter().next().unwrap_or_default(),
            }
        })
        .collect()
}

fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The keys `bench.py` writes into a run record.
fn record_fields() -> BTreeSet<String> {
    let runner = read(&field_dir().join("bench.py"));
    let body = runner
        .split_once("def record_for(")
        .and_then(|(_, rest)| rest.split("\ndef ").next())
        .expect("bench.py no longer builds its record in record_for");
    body.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn test_every_measured_condition_is_one_the_record_carries_and_the_folder_judges() {
    let fields = record_fields();
    let folder = read(&field_dir().join("report.py"));
    let claims = read(&field_dir().join("CLAIMS.md"));
    let prompt = read(&field_dir().join("PROMPT.md"));

    let claim_count = (1..=9)
        .filter(|n| claims.contains(&format!("**C{} —", n)))
        .count();
    let stated = measured_conditions(&claims);
    assert_eq!(
        stated.len(),
        claim_count,
        "every claim in CLAIMS.md must say what it is measured from and what judges it"
    );
    let exit = measured_conditions(&prompt);
    assert!(
        !exit.is_empty(),
        "the exit criterion in PROMPT.md names no measured condition"
    );

    for condition in stated.iter().chain(exit.iter()) {
        for field in &condition.fields {
            assert!(
                fields.contains(field),
                "a condition is stated in terms of `{}`, which bench.py never records, \
                 so its verdict could only be typed by hand",
                field
            );
        }
        assert!(
            folder.contains(&format!("\"{}\":", condition.verdict)),
            "a condition is judged as `{}`, which report.py never computes",
            condition.verdict
        );
    }
}

/// Where the prompt that drives a round names a file, relative to `evals/field/`.
const PROMPT_PATHS: [&str; 4] = ["README.md", "LOG.md", "bench.py", "report.py"];

/// Spellings of an internal planning identifier, as a committed file must never
/// carry one. A reader of this directory has no access to the planning board,
/// so a number from it is a reference they cannot resolve.
///
/// Each spelling anchors on the word that makes it an identifier rather than a
/// quantity. A bare decimal is not listed: a round's log entry reports measured
/// numbers, and a gate that rejected `15.42 seconds` would block the one file
/// every round has to append to.
const PLANNING_NUMBERS: [&str; 7] = [
    "M6.7",
    "T15.",
    "task 15.",
    "tasks 15.",
    "milestone 6",
    "task 16",
    "task 17",
];

#[test]
fn test_the_prompt_that_drives_a_round_is_committed_and_self_sufficient() {
    let prompt = read(&field_dir().join("PROMPT.md"));
    for path in PROMPT_PATHS {
        assert!(
            prompt.contains(path),
            "PROMPT.md does not point the operator at {}",
            path
        );
        assert!(
            field_dir().join(path).exists(),
            "PROMPT.md names {}, which is not in this directory",
            path
        );
    }
    assert!(
        prompt.contains("Do not design a new experiment"),
        "PROMPT.md drops the instruction that makes a re-run a re-run"
    );
    assert!(
        prompt.contains("per-tool verdict"),
        "PROMPT.md does not carry the no-per-tool-verdicts rule into the run"
    );
    assert!(
        prompt.contains("counted separately"),
        "PROMPT.md does not carry the defect-cornering rule into the run"
    );
    assert!(
        prompt.contains("baseline.md") && prompt.contains("never"),
        "PROMPT.md does not say that the superseded numbers are never rewritten"
    );
}

#[test]
fn test_the_benchmark_carries_no_internal_planning_numbers() {
    let mut committed = String::new();
    for entry in fs::read_dir(field_dir()).expect("cannot read the benchmark directory") {
        let path = entry.expect("cannot read a benchmark entry").path();
        if path.file_name().is_some_and(|name| name == "runs") {
            continue;
        }
        if path.is_dir() {
            collect_sources(&path, &mut committed);
        } else if let Ok(contents) = fs::read_to_string(&path) {
            committed.push_str(&contents);
        }
    }
    for number in PLANNING_NUMBERS {
        assert!(
            !committed.contains(number),
            "a committed benchmark file carries the planning identifier {}, \
             which a reader of this directory cannot resolve",
            number
        );
    }
}
