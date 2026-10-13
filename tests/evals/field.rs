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

use crate::skills::recommended_surfaces;
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

/// The jobs of a round: four on the processor, five on the GPU.
const JOBS: [Job; 9] = [
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
    Job {
        id: "06-gpu-life",
        seeded: false,
        min_cases: 5,
    },
    Job {
        id: "07-gpu-escape-time",
        seeded: false,
        min_cases: 5,
    },
    Job {
        id: "08-gpu-histogram",
        seeded: false,
        min_cases: 5,
    },
    Job {
        id: "09-gpu-particles",
        seeded: false,
        min_cases: 5,
    },
];

/// The arms a job can be run under: two cold Miri arms, three taught Miri
/// before the brief, and the two baselines.
const ARMS: [&str; 7] = [
    "miri-pack",
    "miri-bare",
    "miri-skill",
    "miri-grammar",
    "miri-both",
    "python",
    "rust",
];

/// The languages a seeded job commits a starting source for.
const SEED_LANGUAGES: [&str; 3] = ["miri", "python", "rust"];

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
fn test_arms_are_the_ones_the_benchmark_compares() {
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

/// The hidden tests run a program the way its own toolchain runs a project. A
/// subject told to use `uv` installs its packages into the workspace's
/// environment, and a scorer that runs a bare interpreter misses them: the
/// program dies on an import and reads as a wrong answer it never gave.
#[test]
fn test_the_python_arm_is_run_through_its_project_runner() {
    let path = field_dir().join("arms.toml");
    let parsed: ArmsFile = toml::from_str(&read(&path))
        .unwrap_or_else(|e| panic!("cannot parse {}: {}", path.display(), e));
    let python = parsed
        .arm
        .iter()
        .find(|arm| arm.id == "python")
        .expect("no python arm");
    assert!(
        python.toolchain.contains("`uv`"),
        "the python arm's toolchain no longer names uv"
    );
    assert_eq!(
        python.run.iter().take(2).collect::<Vec<_>>(),
        ["uv", "run"],
        "the python arm is scored outside the environment its toolchain built"
    );
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
        claims.contains("1.5"),
        "CLAIMS.md drops C1's bar against the cheaper baseline"
    );
    assert!(
        claims.contains("miri-grammar"),
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
    for script in ["bench.py", "report.py", "usage.py"] {
        assert!(dir.join(script).is_file(), "{} is missing", script);
    }
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the harness scripts were not compiled");
        return;
    }
    for script in ["bench.py", "report.py", "usage.py"] {
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
/// reading standard input, because one of the four languages cannot read
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

/// The jobs the claims are judged over: every job marked `gpu = true`.
const GPU_JOBS: [&str; 5] = [
    "05-gpu-heat",
    "06-gpu-life",
    "07-gpu-escape-time",
    "08-gpu-histogram",
    "09-gpu-particles",
];

/// The languages a Miri arm is compared against.
const BASELINE_ARMS: [&str; 2] = ["python", "rust"];

/// What one synthetic run cost and whether it reached green.
#[derive(Clone, Copy)]
struct SyntheticRun {
    tokens: u32,
    seconds: u32,
    invocations: u32,
    green: bool,
    /// Whether the run declared itself finished on an answer the hidden tests
    /// refuse — the worst thing a round can record.
    silent: bool,
}

impl SyntheticRun {
    fn green(tokens: u32, seconds: u32, invocations: u32) -> Self {
        SyntheticRun {
            tokens,
            seconds,
            invocations,
            green: true,
            silent: false,
        }
    }
}

/// The first run of a cell of a synthetic round.
fn write_synthetic_record(root: &Path, job: &str, arm: &str, run: SyntheticRun) {
    write_synthetic_run(root, job, arm, 1, run);
}

/// One numbered run of a cell of a synthetic round, written where the folder
/// will read it.
fn write_synthetic_run(root: &Path, job: &str, arm: &str, index: u32, run: SyntheticRun) {
    let directory = root
        .join("synthetic")
        .join(job)
        .join(arm)
        .join("claude-sonnet");
    fs::create_dir_all(&directory).expect("cannot create a synthetic round");
    let passed = if run.green { 6 } else { 3 };
    let record = format!(
        r#"{{"schemaVersion":1,"round":"synthetic","job":"{job}","arm":"{arm}","run":{index},
            "model":"claude-sonnet","modelId":"x","harness":{{"name":"claude","version":"1"}},
            "compilerCommit":"0","compilerVersion":"0","packInstalled":false,
            "caps":{{"turns":1,"timeSeconds":1}},"startedAt":"now","wallClockSeconds":{seconds}.0,
            "tokens":{{"in":{half},"out":{half}}},"turns":1,"toolInvocations":{invocations},
            "toolchainInvocations":1,"outcome":"finished",
            "hiddenTests":{{"passed":{passed},"total":6,"failures":[]}},
            "silentWrongAnswer":{silent},"workspace":"/tmp",
            "isolation":{{"clean":true,"initObserved":true,"hookEvents":0,"mcpServers":[],
            "plugins":[],"skills":[],"apiKeySource":"none"}}}}"#,
        job = job,
        arm = arm,
        half = run.tokens / 2,
        seconds = run.seconds,
        invocations = run.invocations,
        passed = passed,
        silent = run.silent,
        index = index,
    );
    fs::write(directory.join(format!("{}.json", index)), record)
        .expect("cannot write a synthetic record");
}

/// A synthetic round over every GPU job: the skill arm at one cost, every
/// baseline at another.
fn write_pack_against_baselines(root: &Path, pack: SyntheticRun, baseline: SyntheticRun) {
    for job in GPU_JOBS {
        write_synthetic_record(root, job, "miri-skill", pack);
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
    let job = "05-gpu-heat";
    write_synthetic_record(&scratch, job, "miri-skill", SyntheticRun::green(1000, 1, 1));
    let unfinished = SyntheticRun {
        green: false,
        ..SyntheticRun::green(3000, 1, 1)
    };
    write_synthetic_record(&scratch, job, "miri-grammar", unfinished);
    let held = fold(&scratch);
    assert!(
        claim_held(&held, "C4"),
        "the judge does not report C4 as held on data that satisfies it:\n{}",
        held
    );

    // Both arms finish every run and the pack is cheaper. A bare arm that
    // already finishes everything leaves no finish rate to beat, so a claim
    // that demanded a strictly higher one could never hold.
    let _ = fs::remove_dir_all(&scratch);
    write_synthetic_record(&scratch, job, "miri-skill", SyntheticRun::green(1000, 1, 1));
    write_synthetic_record(
        &scratch,
        job,
        "miri-grammar",
        SyntheticRun::green(3000, 1, 1),
    );
    let both_finish = fold(&scratch);
    assert!(
        claim_held(&both_finish, "C4"),
        "the judge refuses C4 when both arms finish every run and the pack is cheaper:\n{}",
        both_finish
    );

    // The pack is cheaper on its green run but finishes half as often.
    let _ = fs::remove_dir_all(&scratch);
    write_synthetic_run(
        &scratch,
        job,
        "miri-skill",
        1,
        SyntheticRun::green(1000, 1, 1),
    );
    write_synthetic_run(&scratch, job, "miri-skill", 2, unfinished);
    write_synthetic_record(
        &scratch,
        job,
        "miri-grammar",
        SyntheticRun::green(3000, 1, 1),
    );
    let finishes_less = fold(&scratch);
    assert!(
        !claim_held(&finishes_less, "C4"),
        "the judge reports C4 as held when the pack finishes less often than bare:\n{}",
        finishes_less
    );

    // The same shape with the pack dearer than the bare arm must fail C4, or
    // the verdict is a constant and the article would publish on a constant.
    let _ = fs::remove_dir_all(&scratch);
    write_synthetic_record(&scratch, job, "miri-skill", SyntheticRun::green(4000, 1, 1));
    write_synthetic_record(
        &scratch,
        job,
        "miri-grammar",
        SyntheticRun::green(1000, 1, 1),
    );
    let refused = fold(&scratch);
    assert!(
        !claim_held(&refused, "C4"),
        "the judge reports C4 as held on data that refutes it:\n{}",
        refused
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_the_skill_arm_must_land_within_the_bar_of_the_better_baseline() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-bar-claims");
    let _ = fs::remove_dir_all(&scratch);

    // Cheaper and faster than every baseline in every GPU job.
    write_pack_against_baselines(
        &scratch,
        SyntheticRun::green(1000, 100, 1),
        SyntheticRun::green(1100, 110, 1),
    );
    let leads = fold(&scratch);
    for claim in ["C1", "C5"] {
        assert!(
            claim_held(&leads, claim),
            "the judge does not report {} as held when the skill arm leads:\n{}",
            claim,
            leads
        );
    }
    assert!(leads.contains("\"standing\": \"beats\""));

    // 1.4 times the baselines: inside the bar, and reported as behind, so a
    // held claim never reads as a lead.
    let _ = fs::remove_dir_all(&scratch);
    write_pack_against_baselines(
        &scratch,
        SyntheticRun::green(1400, 140, 1),
        SyntheticRun::green(1000, 100, 1),
    );
    let inside = fold(&scratch);
    for claim in ["C1", "C5"] {
        assert!(
            claim_held(&inside, claim),
            "the judge refuses {} inside the 1.5x bar:\n{}",
            claim,
            inside
        );
    }
    assert!(
        inside.contains("\"standing\": \"behind\""),
        "a result past the parity band is not labelled behind:\n{}",
        inside
    );

    // 1.6 times the baselines: past the bar, so neither claim holds.
    let _ = fs::remove_dir_all(&scratch);
    write_pack_against_baselines(
        &scratch,
        SyntheticRun::green(1600, 160, 1),
        SyntheticRun::green(1000, 100, 1),
    );
    let past = fold(&scratch);
    for claim in ["C1", "C5"] {
        assert!(
            !claim_held(&past, claim),
            "the judge reports {} as held past the 1.5x bar:\n{}",
            claim,
            past
        );
    }

    // A baseline that was never run is not a baseline the skill arm reached.
    let _ = fs::remove_dir_all(&scratch);
    for job in GPU_JOBS {
        write_synthetic_record(&scratch, job, "miri-skill", SyntheticRun::green(1, 1, 1));
    }
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
fn test_the_exit_criterion_compares_the_skill_loop_against_the_grammar_loop() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-pack-loop");
    let _ = fs::remove_dir_all(&scratch);
    let job = "05-gpu-heat";

    write_synthetic_record(&scratch, job, "miri-skill", SyntheticRun::green(1, 1, 10));
    write_synthetic_record(&scratch, job, "miri-grammar", SyntheticRun::green(1, 1, 12));
    let held = fold(&scratch);
    assert!(
        claim_held(&held, "packLoop"),
        "the judge does not report the pack loop as no worse than the bare loop:\n{}",
        held
    );

    let _ = fs::remove_dir_all(&scratch);
    write_synthetic_record(&scratch, job, "miri-skill", SyntheticRun::green(1, 1, 14));
    write_synthetic_record(&scratch, job, "miri-grammar", SyntheticRun::green(1, 1, 12));
    let refused = fold(&scratch);
    assert!(
        !claim_held(&refused, "packLoop"),
        "the judge reports the pack loop as held when it costs more invocations:\n{}",
        refused
    );
    let _ = fs::remove_dir_all(&scratch);
}

/// The cell every probe test runs: a baseline arm, so no skill pack is installed.
const PROBE_JOB: &str = "01-word-frequency";

/// Stand-ins for the agent harness and the compiler, on a directory put first
/// on `PATH`.
///
/// The harness writes `ratings` as the subject's `RATINGS.json`, or writes
/// nothing when `ratings` is `None`, and streams one usage event. Nothing a
/// probe does is a live model's behaviour, so nothing about one is left to a
/// live model here.
#[cfg(unix)]
fn install_fake_harness(bin: &Path, ratings: Option<&str>) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(bin).expect("cannot create the fake harness directory");
    let write_ratings = ratings
        .map(|text| format!("cat > RATINGS.json <<'RATINGS'\n{}\nRATINGS\n", text))
        .unwrap_or_default();
    let claude = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo 'fake harness 1'; exit 0; fi\n\
         {}echo '{{\"type\":\"assistant\",\"message\":{{\"usage\":{{\"input_tokens\":3,\"output_tokens\":4}},\"content\":[]}}}}'\n",
        write_ratings
    );
    let miri = "#!/bin/sh\necho 'miri 0.0.0-fake'\n";
    for (name, script) in [("claude", claude.as_str()), ("miri", miri)] {
        let path = bin.join(name);
        fs::write(&path, script).expect("cannot write a fake harness");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("cannot make a fake harness executable");
    }
}

/// Run `bench.py` against a synthetic round under `root`, with the fake harness
/// first on `PATH`.
fn run_bench(root: &Path, extra: &[&str]) -> std::process::Output {
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("python3")
        .arg(field_dir().join("bench.py"))
        .args([
            "--job",
            PROBE_JOB,
            "--arm",
            "python",
            "--model",
            "claude-sonnet",
        ])
        .args(["--round", "synthetic", "--records-root"])
        .arg(root.join("runs"))
        .arg("--scratch")
        .arg(root.join("scratch"))
        .args(extra)
        .env("PATH", path)
        .output()
        .expect("cannot run python3")
}

/// The JSON files under a directory, at any depth.
fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for path in entries.filter_map(|entry| entry.ok().map(|e| e.path())) {
        if path.is_dir() {
            found.extend(json_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "json") {
            found.push(path);
        }
    }
    found
}

#[cfg(unix)]
#[test]
fn test_a_probe_record_never_reaches_the_folder() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the probe was not run");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-probe-isolation");
    let _ = fs::remove_dir_all(&root);
    install_fake_harness(
        &root.join("bin"),
        Some(r#"{"ratings": [{"surface": "pytest", "score": 2, "reason": "slow"}]}"#),
    );
    let runs = root.join("runs");
    write_synthetic_record(&runs, PROBE_JOB, "python", SyntheticRun::green(10, 1, 1));
    let measured_workspace = root
        .join("scratch")
        .join("synthetic")
        .join(PROBE_JOB)
        .join("python")
        .join("claude-sonnet")
        .join("1");
    fs::create_dir_all(&measured_workspace).expect("cannot create a measured workspace");
    fs::write(measured_workspace.join("main.py"), "measured").expect("cannot seed it");

    let probed = run_bench(&root, &["--probe"]);
    assert!(
        probed.status.success(),
        "the probe failed:\n{}",
        String::from_utf8_lossy(&probed.stderr)
    );

    // Where the probe wrote, and what it did not touch.
    let probe_record = runs
        .join("synthetic.probe")
        .join(PROBE_JOB)
        .join("python")
        .join("claude-sonnet")
        .join("1.json");
    let written = read(&probe_record);
    assert!(
        written.contains("\"pytest\"") && written.contains("\"score\": 2"),
        "the probe record does not carry the ratings the subject wrote:\n{}",
        written
    );
    assert_eq!(
        json_files(&runs.join("synthetic")).len(),
        1,
        "the probe wrote a file into the round it must stay out of"
    );
    assert_eq!(
        read(&measured_workspace.join("main.py")),
        "measured",
        "the probe overwrote the workspace a measured record points at"
    );

    // The folder reads the round and sees only the measured record.
    let summary = fold(&runs);
    assert!(
        summary.contains("\"records\": 1,"),
        "the folder counted something other than the one measured record:\n{}",
        summary
    );

    // A probe record carried into the round by hand is refused, not folded.
    let smuggled = runs
        .join("synthetic")
        .join(PROBE_JOB)
        .join("python")
        .join("claude-sonnet")
        .join("2.json");
    fs::copy(&probe_record, &smuggled).expect("cannot copy the probe record");
    let refused = Command::new("python3")
        .arg(field_dir().join("report.py"))
        .args(["--round", "synthetic", "--runs-root"])
        .arg(&runs)
        .arg("--out")
        .arg(root.join("contaminated.json"))
        .output()
        .expect("cannot run python3");
    assert!(
        !refused.status.success() && String::from_utf8_lossy(&refused.stderr).contains("probe"),
        "the folder accepted a probe record inside a round:\n{}",
        String::from_utf8_lossy(&refused.stdout)
    );
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn test_a_probe_refuses_ratings_it_cannot_read() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the probe was not run");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-probe-unreadable");
    let unreadable = [
        (
            Some(r#"{"ratings": [{"surface": "pytest", "score": 7}]}"#),
            "score",
        ),
        (Some(r#"{"ratings": []}"#), "no surface"),
        (None, "RATINGS.json"),
    ];
    for (ratings, problem) in unreadable {
        let _ = fs::remove_dir_all(&root);
        install_fake_harness(&root.join("bin"), ratings);
        let probed = run_bench(&root, &["--probe"]);
        let stderr = String::from_utf8_lossy(&probed.stderr);
        assert!(
            !probed.status.success() && stderr.contains(problem),
            "a probe with unreadable ratings ({}) did not fail naming the problem:\n{}",
            problem,
            stderr
        );
        let record = read(
            &root
                .join("runs")
                .join("synthetic.probe")
                .join(PROBE_JOB)
                .join("python")
                .join("claude-sonnet")
                .join("1.json"),
        );
        assert!(
            record.contains("\"ratings\": null") && record.contains("\"ratingsProblem\""),
            "an unreadable probe does not keep its record with the problem named:\n{}",
            record
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_the_harness_command_is_the_one_launched_and_recorded() {
    // An operator may reach the harness through a wrapper of their own, for
    // instance to pick an account. The wrapper must be what runs, and the
    // record must say so; a plain `claude` on the path is a decoy that fails.
    use std::os::unix::fs::PermissionsExt;
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-harness-command");
    let _ = fs::remove_dir_all(&root);
    let bin = root.join("bin");
    install_fake_harness(
        &bin,
        Some(r#"{"ratings": [{"surface": "pytest", "score": 4, "reason": "fine"}]}"#),
    );
    fs::rename(bin.join("claude"), bin.join("personal-claude")).expect("cannot rename");
    let decoy = bin.join("claude");
    fs::write(&decoy, "#!/bin/sh\necho 'decoy launched' >&2\nexit 3\n").expect("cannot write");
    fs::set_permissions(&decoy, fs::Permissions::from_mode(0o755)).expect("cannot chmod");

    let dry = run_bench(
        &root,
        &["--dry-run", "--harness-command", "personal-claude"],
    );
    assert!(
        String::from_utf8_lossy(&dry.stdout).contains("launch     personal-claude -p"),
        "the dry run renders a launch through another command:\n{}",
        String::from_utf8_lossy(&dry.stdout)
    );

    let probed = run_bench(&root, &["--probe", "--harness-command", "personal-claude"]);
    assert!(
        probed.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&probed.stderr)
    );
    let record = read(
        &root
            .join("runs")
            .join("synthetic.probe")
            .join(PROBE_JOB)
            .join("python")
            .join("claude-sonnet")
            .join("1.json"),
    );
    assert!(
        record.contains("\"command\": \"personal-claude\"")
            && record.contains("\"version\": \"fake harness 1\""),
        "the record does not name the command that ran:\n{}",
        record
    );
    let _ = fs::remove_dir_all(&root);
}

/// A stand-in harness that streams `events` (one JSON object per line) and
/// writes the environment it was launched with to `env.txt` in its workspace.
#[cfg(unix)]
fn install_scripted_harness(bin: &Path, events: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    install_fake_harness(bin, None);
    let mut script = String::from(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fake harness 1'; exit 0; fi\nenv > env.txt\n",
    );
    for event in events {
        script.push_str(&format!("echo '{}'\n", event));
    }
    let path = bin.join("claude");
    fs::write(&path, script).expect("cannot write a scripted harness");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("cannot make a scripted harness executable");
}

/// The measured record of the first run of the synthetic python cell.
fn measured_record(root: &Path) -> String {
    read(
        &root
            .join("runs")
            .join("synthetic")
            .join(PROBE_JOB)
            .join("python")
            .join("claude-sonnet")
            .join("1.json"),
    )
}

const CLEAN_INIT: &str = r#"{"type":"system","subtype":"init","apiKeySource":"none","mcp_servers":[],"plugins":[{"name":"core","path":"builtin"}],"skills":["miri-lang"]}"#;

#[test]
fn test_a_subject_is_launched_apart_from_the_operators_configuration() {
    // The operator's CLAUDE.md files, hooks, plugins and MCP servers are not
    // part of any arm. A hook that rewrites shell output would change the very
    // bytes the benchmark measures, and not equally across languages.
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the launch was not rendered");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-isolated-launch");
    let _ = fs::remove_dir_all(&root);
    let dry = run_bench(&root, &["--dry-run"]);
    let launch = String::from_utf8_lossy(&dry.stdout);
    let settings = field_dir().join("harness-settings.json");
    for flag in [
        "--setting-sources project,local".to_string(),
        "--strict-mcp-config".to_string(),
        "--no-chrome".to_string(),
        format!("--settings {}", settings.display()),
    ] {
        assert!(
            launch.contains(&flag),
            "the launch lacks `{}`:\n{}",
            flag,
            launch
        );
    }
    let shipped = read(&settings);
    assert!(
        shipped.contains("\"forceLoginMethod\": \"claudeai\"") && !shipped.contains("hooks"),
        "the benchmark's settings must pin the subscription login and carry no hook:\n{}",
        shipped
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_a_workspace_below_a_claude_md_is_refused() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the launch was not rendered");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-ancestor-memory");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("cannot create the root");
    fs::write(root.join("CLAUDE.md"), "operator instructions").expect("cannot plant it");
    let dry = run_bench(&root, &["--dry-run"]);
    assert!(
        !dry.status.success(),
        "a workspace under a CLAUDE.md was prepared"
    );
    assert!(
        String::from_utf8_lossy(&dry.stderr)
            .contains(&root.join("CLAUDE.md").display().to_string()),
        "the refusal does not name the file:\n{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_the_subject_gets_no_harness_routing_and_its_isolation_is_recorded() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-subject-environment");
    let _ = fs::remove_dir_all(&root);
    install_scripted_harness(&root.join("bin"), &[CLEAN_INIT]);
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("python3")
        .arg(field_dir().join("bench.py"))
        .args([
            "--job",
            PROBE_JOB,
            "--arm",
            "python",
            "--model",
            "claude-sonnet",
        ])
        .args(["--round", "synthetic", "--records-root"])
        .arg(root.join("runs"))
        .arg("--scratch")
        .arg(root.join("scratch"))
        .env("PATH", path)
        .env("CLAUDE_CODE_USE_BEDROCK", "1")
        .env("AWS_PROFILE", "company")
        .env("ANTHROPIC_DEFAULT_SONNET_MODEL", "elsewhere")
        .env("MAX_THINKING_TOKENS", "1")
        .output()
        .expect("cannot run python3");
    assert!(
        output.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let seen = read(
        &root
            .join("scratch")
            .join("synthetic")
            .join(PROBE_JOB)
            .join("python")
            .join("claude-sonnet")
            .join("1")
            .join("env.txt"),
    );
    for name in [
        "CLAUDE_CODE_USE_BEDROCK",
        "AWS_PROFILE",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "MAX_THINKING_TOKENS",
    ] {
        assert!(!seen.contains(name), "the subject inherited {}", name);
    }
    let repository = field_dir()
        .parent()
        .and_then(Path::parent)
        .expect("the field directory sits two levels below the repository")
        .display()
        .to_string();
    assert!(
        !seen.contains(&repository),
        "the subject's environment names the compiler repository:\n{}",
        seen
    );
    assert!(
        seen.contains("MIRI_RUNTIME_DIR="),
        "the subject was not handed a staged runtime"
    );
    let staged = fs::read_dir(root.join("scratch").join("toolchain"))
        .expect("no toolchain directory was staged")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("bin").join("miri").is_file())
        .expect("no staged toolchain carries a compiler");
    assert!(
        staged.join("lib").join("miri").join("stdlib").is_dir(),
        "the staged toolchain carries no standard library"
    );
    assert!(
        seen.contains(&format!(
            "MIRI_RUNTIME_DIR={}",
            staged.join("runtime").display()
        )),
        "the subject's runtime directory is not the staged one"
    );
    let record = measured_record(&root);
    assert!(
        record.contains("\"clean\": true") && record.contains("\"miri-lang\""),
        "the record does not carry the isolation the harness reported:\n{}",
        record
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_a_run_a_hook_touched_is_recorded_unclean_and_never_folded() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-hooked-run");
    let _ = fs::remove_dir_all(&root);
    install_scripted_harness(
        &root.join("bin"),
        &[
            CLEAN_INIT,
            r#"{"type":"system","subtype":"hook_started","hook_event":"PreToolUse"}"#,
        ],
    );
    let ran = run_bench(&root, &[]);
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let record = measured_record(&root);
    assert!(
        record.contains("\"clean\": false") && record.contains("\"hookEvents\": 1"),
        "a hooked run is not recorded as unclean:\n{}",
        record
    );
    let folded = Command::new("python3")
        .arg(field_dir().join("report.py"))
        .args(["--round", "synthetic", "--runs-root"])
        .arg(root.join("runs"))
        .arg("--out")
        .arg(root.join("summary.json"))
        .output()
        .expect("cannot run python3");
    assert!(
        !folded.status.success(),
        "the folder folded a run a hook touched"
    );
    assert!(
        String::from_utf8_lossy(&folded.stderr).contains("not isolated"),
        "the refusal does not say why:\n{}",
        String::from_utf8_lossy(&folded.stderr)
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_a_run_that_reached_into_the_repository_is_not_clean() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-repository-reach");
    let _ = fs::remove_dir_all(&root);
    let repository = field_dir()
        .parent()
        .and_then(Path::parent)
        .expect("the field directory sits two levels below the repository")
        .display()
        .to_string();
    let reach = format!(
        r#"{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"tool_use","name":"Bash","input":{{"command":"ls {}/docs"}}}}]}}}}"#,
        repository
    );
    install_scripted_harness(&root.join("bin"), &[CLEAN_INIT, reach.as_str()]);
    let ran = run_bench(&root, &[]);
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let record = measured_record(&root);
    assert!(
        record.contains("\"clean\": false") && record.contains("\"repoReferences\": 1"),
        "a run that read the compiler repository is recorded clean:\n{}",
        record
    );
    let _ = fs::remove_dir_all(&root);
}

/// One model response streamed the way the `claude` harness streams it: one
/// event per content block, every block repeating the message's id and the
/// partial usage it had when the message started.
const SPLIT_RESPONSE: [&str; 3] = [
    r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"plan"}],"usage":{"input_tokens":2,"cache_read_input_tokens":900,"output_tokens":2}}}"#,
    r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"checking"}],"usage":{"input_tokens":2,"cache_read_input_tokens":900,"output_tokens":2}}}"#,
    r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":2,"cache_read_input_tokens":900,"output_tokens":2}}}"#,
];

#[test]
#[cfg(unix)]
fn test_a_turn_is_a_message_and_tokens_are_what_the_harness_settled() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-settled-usage");
    let _ = fs::remove_dir_all(&root);
    let mut events = vec![CLEAN_INIT];
    events.extend(SPLIT_RESPONSE);
    events.push(
        r#"{"type":"result","subtype":"success","num_turns":1,"usage":{"input_tokens":5,"cache_creation_input_tokens":40,"cache_read_input_tokens":900,"output_tokens":70}}"#,
    );
    install_scripted_harness(&root.join("bin"), &events);
    let ran = run_bench(&root, &[]);
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let record = measured_record(&root);
    for expected in [
        "\"turns\": 1,",
        "\"toolInvocations\": 1,",
        "\"in\": 945,",
        "\"out\": 70,",
        "\"cacheRead\": 900,",
        "\"cacheCreation\": 40,",
        "\"observed\": true",
    ] {
        assert!(
            record.contains(expected),
            "the record does not carry {}:\n{}",
            expected,
            record
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_a_failed_hidden_case_keeps_its_exit_status_and_stderr() {
    if !available("python3") || !available("uv") {
        println!("skipping: python3 or uv is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-failure-detail");
    let _ = fs::remove_dir_all(&root);
    install_scripted_harness(&root.join("bin"), &[CLEAN_INIT]);
    let ran = run_bench(&root, &[]);
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let record = measured_record(&root);
    assert!(
        record.contains("\"failureDetail\"")
            && record.contains("\"exitCode\": 2")
            && record.contains("main.py"),
        "a failed case does not say why it failed:\n{}",
        record
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_a_rescored_record_keeps_the_score_it_replaced() {
    if !available("python3") || !available("uv") {
        println!("skipping: python3 or uv is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-rescore");
    let _ = fs::remove_dir_all(&root);
    install_scripted_harness(&root.join("bin"), &[CLEAN_INIT]);
    let ran = run_bench(&root, &[]);
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(
        measured_record(&root).contains("\"silentWrongAnswer\": true"),
        "a workspace with no program was not scored as a wrong answer"
    );
    let workspace = root
        .join("scratch")
        .join("synthetic")
        .join(PROBE_JOB)
        .join("python")
        .join("claude-sonnet")
        .join("1");
    fs::copy(
        field_dir()
            .join("jobs")
            .join(PROBE_JOB)
            .join("reference")
            .join("solution.py"),
        workspace.join("main.py"),
    )
    .expect("cannot place the reference program");
    let record_path = root
        .join("runs")
        .join("synthetic")
        .join(PROBE_JOB)
        .join("python")
        .join("claude-sonnet")
        .join("1.json");
    let rescored = Command::new("python3")
        .arg(field_dir().join("bench.py"))
        .args(["--job", PROBE_JOB, "--arm", "python", "--rescore"])
        .arg(&record_path)
        .output()
        .expect("cannot run python3");
    assert!(
        rescored.status.success(),
        "the rescore failed:\n{}",
        String::from_utf8_lossy(&rescored.stderr)
    );
    let record = read(&record_path);
    assert!(
        record.contains("\"silentWrongAnswer\": false")
            && record.contains("\"rescored\"")
            && record.contains("\"previous\"")
            && record.contains("\"silentWrongAnswer\": true"),
        "the rescore did not replace the score and keep the old one:\n{}",
        record
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn test_a_stream_that_never_settled_leaves_its_tokens_unobserved() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-unsettled-usage");
    let _ = fs::remove_dir_all(&root);
    let mut events = vec![CLEAN_INIT];
    events.extend(SPLIT_RESPONSE);
    install_scripted_harness(&root.join("bin"), &events);
    let ran = run_bench(&root, &[]);
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let record = measured_record(&root);
    assert!(
        record.contains("\"observed\": false") && record.contains("\"turns\": 1,"),
        "partial per-block usage was read as a settled cost:\n{}",
        record
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_only_a_probe_is_asked_for_ratings() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the launch was not rendered");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-probe-prompt");
    let _ = fs::remove_dir_all(&root);
    let measured = run_bench(&root, &["--dry-run"]);
    let probe = run_bench(&root, &["--dry-run", "--probe"]);
    for output in [&measured, &probe] {
        assert!(
            output.status.success(),
            "a dry run failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(
        !String::from_utf8_lossy(&measured.stdout).contains("RATINGS.json"),
        "a measured arm is asked for per-tool ratings"
    );
    assert!(
        String::from_utf8_lossy(&probe.stdout).contains("RATINGS.json"),
        "a probe is launched without asking for the ratings it exists to collect"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_a_probe_asks_for_ratings_keyed_on_the_committed_surfaces() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the launch was not rendered");
        return;
    }
    // A verdict computed by joining ratings against a vocabulary is only as
    // good as the subject's spelling. Handing it the names is what makes
    // `miri check` and `the check command` the same answer.
    let root = std::env::temp_dir().join("miri-field-probe-vocabulary");
    let _ = fs::remove_dir_all(&root);
    let probe = run_bench(&root, &["--dry-run", "--probe"]);
    assert!(
        probe.status.success(),
        "the probe dry run failed:\n{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let launch = String::from_utf8_lossy(&probe.stdout);
    for surface in recommended_surfaces().all() {
        assert!(
            launch.contains(surface),
            "the probe never names `{}`, so a subject would spell it however it liked \
             and the verdict would join on nothing:\n{}",
            surface,
            launch
        );
    }
    let _ = fs::remove_dir_all(&root);
}

/// One probe record of a synthetic round, written beside the round rather than
/// inside it, where the folder reads a probe from.
fn write_synthetic_probe(root: &Path, arm: &str, ratings: &str, problem: &str) {
    let directory = root
        .join("synthetic.probe")
        .join(PROBE_JOB)
        .join(arm)
        .join("claude-sonnet");
    fs::create_dir_all(&directory).expect("cannot create a synthetic probe");
    let record = format!(
        r#"{{"schemaVersion":1,"kind":"probe","round":"synthetic","job":"{job}","arm":"{arm}",
            "run":1,"model":"claude-sonnet","modelId":"x",
            "harness":{{"name":"claude","version":"1"}},"compilerCommit":"0",
            "compilerVersion":"0","packInstalled":true,"startedAt":"now","outcome":"finished",
            "ratings":{ratings},"ratingsProblem":{problem},"workspace":"/tmp"}}"#,
        job = PROBE_JOB,
        arm = arm,
        ratings = ratings,
        problem = problem,
    );
    fs::write(directory.join("1.json"), record).expect("cannot write a synthetic probe record");
}

/// A rating of one surface, as a subject writes it.
fn rating(surface: &str, score: u32) -> String {
    format!(
        r#"{{"surface":"{}","score":{},"reason":"r"}}"#,
        surface, score
    )
}

/// A synthetic round with one measured record, so the folder has a round to fold.
fn seed_round(root: &Path) {
    write_synthetic_record(root, PROBE_JOB, "miri-pack", SyntheticRun::green(1, 1, 1));
    write_synthetic_record(root, PROBE_JOB, "miri-bare", SyntheticRun::green(1, 1, 1));
}

/// A scratch round for one probe case, cleared and seeded.
fn probe_scratch(case: &str) -> PathBuf {
    let scratch = std::env::temp_dir().join(format!("miri-field-probe-{}", case));
    let _ = fs::remove_dir_all(&scratch);
    seed_round(&scratch);
    scratch
}

/// The first command of the vocabulary, as a surface to rate.
fn a_recommended_surface() -> String {
    recommended_surfaces()
        .commands
        .first()
        .expect("the vocabulary lists no command")
        .clone()
}

#[test]
fn test_a_probe_that_rated_every_surface_well_holds_the_opinion_condition() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = probe_scratch("well-rated");
    let surfaces = recommended_surfaces();
    let rated = surfaces
        .all()
        .iter()
        .map(|surface| rating(surface, 4))
        .collect::<Vec<_>>()
        .join(",");
    write_synthetic_probe(&scratch, "miri-pack", &format!("[{}]", rated), "null");

    let held = fold(&scratch);
    assert!(
        claim_held(&held, "probeRatings"),
        "the judge does not report a well-rated probe as holding:\n{}",
        held
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_a_surface_rated_at_the_bar_fails_the_opinion_condition_and_is_named() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = probe_scratch("rated-low");
    let surface = a_recommended_surface();
    write_synthetic_probe(
        &scratch,
        "miri-pack",
        &format!("[{},{}]", rating(&surface, 2), rating("miri test", 5)),
        "null",
    );

    let refused = fold(&scratch);
    assert!(
        !claim_held(&refused, "probeRatings"),
        "the judge reports a surface rated at the bar as holding:\n{}",
        refused
    );
    assert!(
        refused.contains(&surface),
        "the verdict does not name the surface that was rated low:\n{}",
        refused
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_a_surface_is_judged_on_its_worst_score_however_it_was_spelled() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    // The subject types the name, so case and spacing must decide nothing, and
    // one arm liking a surface must not cancel another arm's verdict on it.
    let scratch = probe_scratch("worst-score");
    let surface = a_recommended_surface();
    write_synthetic_probe(
        &scratch,
        "miri-pack",
        &format!("[{}]", rating(&surface.to_uppercase(), 5)),
        "null",
    );
    write_synthetic_probe(
        &scratch,
        "miri-bare",
        &format!("[{}]", rating(&format!("  {}  ", surface), 2)),
        "null",
    );

    let disagreed = fold(&scratch);
    assert!(
        !claim_held(&disagreed, "probeRatings"),
        "a surface one probe rated at the bar reads as holding because another liked it:\n{}",
        disagreed
    );
    assert!(
        !disagreed.contains("unrecognised\": [\n"),
        "a surface the subject typed in another case placed nowhere, \
         so the join is spelling-bound:\n{}",
        disagreed
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_an_opinion_condition_nobody_answered_does_not_hold() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    // Three ways to answer nothing. Each must fail: a condition that held on
    // silence would be held by having asked no one.
    let unprobed = probe_scratch("unprobed");
    let folded = fold(&unprobed);
    assert!(
        !claim_held(&folded, "probeRatings"),
        "the judge reports a round that was never probed as holding:\n{}",
        folded
    );
    let _ = fs::remove_dir_all(&unprobed);

    let unjoined = probe_scratch("unjoined");
    write_synthetic_probe(
        &unjoined,
        "python",
        &format!("[{}]", rating("pytest", 5)),
        "null",
    );
    let folded = fold(&unjoined);
    assert!(
        !claim_held(&folded, "probeRatings"),
        "the judge reports a probe that rated no recommended surface as holding:\n{}",
        folded
    );
    assert!(
        folded.contains("pytest"),
        "the verdict drops a surface it could not place, so a misspelling would be invisible:\n{}",
        folded
    );
    let _ = fs::remove_dir_all(&unjoined);

    let unreadable = probe_scratch("rated-nothing");
    write_synthetic_probe(
        &unreadable,
        "miri-pack",
        "null",
        r#""RATINGS.json is missing""#,
    );
    let folded = fold(&unreadable);
    assert!(
        !claim_held(&folded, "probeRatings"),
        "the judge reports a probe whose ratings could not be read as holding:\n{}",
        folded
    );
    let _ = fs::remove_dir_all(&unreadable);
}

#[test]
fn test_a_measured_record_kept_beside_a_probe_is_refused() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    // The isolation runs both ways. A probe record inside a round is already
    // refused; a run record beside a probe means the same hand mixed the two.
    let scratch = probe_scratch("mixed");
    let cell = |round: &str| {
        scratch
            .join(round)
            .join(PROBE_JOB)
            .join("miri-pack")
            .join("claude-sonnet")
    };
    fs::create_dir_all(cell("synthetic.probe")).expect("cannot create a probe directory");
    fs::copy(
        cell("synthetic").join("1.json"),
        cell("synthetic.probe").join("1.json"),
    )
    .expect("cannot copy a measured record");

    let mixed = Command::new("python3")
        .arg(field_dir().join("report.py"))
        .args(["--round", "synthetic", "--runs-root"])
        .arg(&scratch)
        .arg("--out")
        .arg(scratch.join("mixed.json"))
        .output()
        .expect("cannot run python3");
    assert!(
        !mixed.status.success()
            && String::from_utf8_lossy(&mixed.stderr).contains("measured run record"),
        "the folder accepted a measured record kept beside a probe:\n{}",
        String::from_utf8_lossy(&mixed.stdout)
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_the_exit_criterion_refuses_a_round_that_produced_a_silent_wrong_answer() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the folder was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-silent-wrong-answers");

    let _ = fs::remove_dir_all(&scratch);
    seed_round(&scratch);
    let clean = fold(&scratch);
    assert!(
        claim_held(&clean, "silentWrongAnswers"),
        "the judge does not report a round with no silent wrong answer as holding:\n{}",
        clean
    );

    let _ = fs::remove_dir_all(&scratch);
    seed_round(&scratch);
    write_synthetic_run(
        &scratch,
        PROBE_JOB,
        "miri-pack",
        2,
        SyntheticRun {
            silent: true,
            ..SyntheticRun::green(1, 1, 1)
        },
    );
    let refused = fold(&scratch);
    assert!(
        !claim_held(&refused, "silentWrongAnswers"),
        "the judge reports a round whose Miri arm answered wrongly in silence as holding:\n{}",
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

/// The keys `bench.py` writes into a record, measured or probe.
///
/// A condition may be stated in terms of either. The opinion condition reads a
/// probe's `ratings`, which no measured run carries and no round folds, and it
/// is a recorded number all the same.
fn record_fields() -> BTreeSet<String> {
    let runner = read(&field_dir().join("bench.py"));
    ["def record_for(", "def probe_record("]
        .into_iter()
        .flat_map(|builder| fields_built_by(&runner, builder))
        .collect()
}

/// The keys one of `bench.py`'s record builders writes.
fn fields_built_by(runner: &str, builder: &str) -> Vec<String> {
    let fields = runner
        .split_once(builder)
        .and_then(|(_, rest)| rest.split("\ndef ").next())
        .and_then(|body| body.split_once("return {"))
        .map(|(_, fields)| fields)
        .unwrap_or_else(|| panic!("bench.py no longer builds a record in {}", builder));
    fields
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The verdict keys `report.py` publishes under one object of its summary.
///
/// Read from the object rather than from the whole file: `silentWrongAnswers`
/// is also the name of a per-cell count, and a gate satisfied by finding that
/// spelling anywhere would pass for a condition no verdict is computed for.
fn published_keys(folder: &str, object: &str) -> BTreeSet<String> {
    let (_, rest) = folder
        .split_once(&format!("\"{}\": {{", object))
        .unwrap_or_else(|| panic!("report.py no longer publishes a {} object", object));
    let mut keys = BTreeSet::new();
    for line in rest.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.starts_with('}') {
            break;
        }
        if let Some((key, _)) = trimmed
            .strip_prefix('"')
            .and_then(|key| key.split_once('"'))
        {
            keys.insert(key.to_string());
        }
    }
    keys
}

/// Where the exit criterion's list of conditions opens and closes in `PROMPT.md`.
const EXIT_LIST_OPENS: &str = "It ends on the first round in which all three";
const EXIT_LIST_CLOSES: &str = "If any of the three fails";

/// The exit criterion's conditions, one string per bullet.
///
/// A condition wrapped across lines is joined back into one, so how the prose
/// was reflowed decides nothing.
fn exit_conditions(prompt: &str) -> Vec<String> {
    let block = prompt
        .split_once(EXIT_LIST_OPENS)
        .and_then(|(_, rest)| rest.split_once(EXIT_LIST_CLOSES))
        .map(|(block, _)| block)
        .unwrap_or_else(|| {
            panic!(
                "PROMPT.md no longer lists the exit criterion between `{}` and `{}`",
                EXIT_LIST_OPENS, EXIT_LIST_CLOSES
            )
        });
    let mut conditions: Vec<String> = Vec::new();
    for line in block.lines() {
        let trimmed = line.trim();
        match (trimmed.strip_prefix("- "), conditions.last_mut()) {
            (Some(opening), _) => conditions.push(opening.to_string()),
            (None, Some(current)) if !trimmed.is_empty() => {
                current.push(' ');
                current.push_str(trimmed);
            }
            (None, _) => {}
        }
    }
    conditions
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
    // Every condition of the exit criterion, not merely one of them: a
    // condition that names no computed verdict is a verdict typed by hand, and
    // a hand-typed verdict is the thing this gate exists to make impossible.
    let conditions = exit_conditions(&prompt);
    assert!(
        !conditions.is_empty(),
        "the exit criterion in PROMPT.md lists no condition"
    );
    let mut exit = Vec::new();
    for condition in &conditions {
        let measured = measured_conditions(condition);
        assert_eq!(
            measured.len(),
            1,
            "an exit condition states no `Measured from ...; judged as ...` sentence, \
             so its verdict could only be typed by hand: {}",
            condition
        );
        exit.extend(measured);
    }

    let published = [
        (&stated, published_keys(&folder, "claims")),
        (&exit, published_keys(&folder, "exitCriterion")),
    ];
    for (conditions, keys) in &published {
        for condition in conditions.iter() {
            for field in &condition.fields {
                assert!(
                    fields.contains(field),
                    "a condition is stated in terms of `{}`, which bench.py never records, \
                     so its verdict could only be typed by hand",
                    field
                );
            }
            assert!(
                keys.contains(&condition.verdict),
                "a condition is judged as `{}`, which report.py never publishes a verdict under",
                condition.verdict
            );
        }
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
        prompt.contains("--probe") && prompt.contains(".probe/"),
        "PROMPT.md does not say how the opinion probe is run or where its ratings land, \
         so a probe would be launched by hand, outside the instrument"
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

/// Runs `code` with `bench.py` imported as `bench`, and returns what it printed.
fn with_bench(code: &str) -> String {
    let output = Command::new("python3")
        .arg("-c")
        .arg(format!(
            "import sys\nsys.path.insert(0, {:?})\nimport bench\n{}",
            field_dir().display().to_string(),
            code
        ))
        .output()
        .expect("cannot run python3");
    assert!(
        output.status.success(),
        "the scorer failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The comparison a numeric job declares: 1% of a value, or 0.5 near zero,
/// and a labelled line held to 0.001.
const NUMERIC: &str = r#"{"kind": "numeric", "relative": 0.01, "absolute": 0.5, "labels": {"MEAN": {"relative": 0.0, "absolute": 0.001}}}"#;

#[test]
fn test_a_numeric_job_accepts_values_within_its_tolerance() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the scorer was not run");
        return;
    }
    let verdict = with_bench(&format!(
        "print(bench.first_difference('COUNT 1000 7\\nMEAN 2.500', 'COUNT 1009 7.4\\nMEAN 2.5005', {}))",
        NUMERIC
    ));
    assert_eq!(verdict, "None", "values inside the tolerance were refused");
}

#[test]
fn test_a_numeric_job_refuses_a_value_outside_its_tolerance_and_names_it() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the scorer was not run");
        return;
    }
    let verdict = with_bench(&format!(
        "print(bench.first_difference('COUNT 1000 7\\nMEAN 2.500', 'COUNT 1000 7\\nMEAN 2.502', {}))",
        NUMERIC
    ));
    assert_eq!(
        verdict, "{'line': 2, 'field': 2, 'expected': '2.500', 'actual': '2.502'}",
        "a labelled line was not held to its own tolerance, or the difference was not named"
    );
    let words = with_bench(&format!(
        "print(bench.first_difference('COUNT 1000', 'TOTAL 1000', {}))",
        NUMERIC
    ));
    assert!(
        words.contains("'expected': 'COUNT'"),
        "a word was compared within a tolerance: {}",
        words
    );
}

#[test]
fn test_an_exact_job_names_the_first_line_that_differs() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the scorer was not run");
        return;
    }
    let verdict = with_bench("print(bench.first_difference('a\\nb', 'a\\nc', {}))");
    assert_eq!(verdict, "{'line': 2, 'expected': 'b', 'actual': 'c'}");
}

/// Score `program` as the python arm's answer to the heat job, with `files`
/// beside it, and return the score.
fn score_heat_workspace(case: &str, program: &Path, files: &[(&str, &str)]) -> String {
    let workspace = std::env::temp_dir().join(format!("miri-field-kernel-{}", case));
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(&workspace).expect("cannot create a workspace");
    fs::copy(program, workspace.join("main.py")).expect("cannot place the program");
    for (name, text) in files {
        fs::write(workspace.join(name), text).expect("cannot write a workspace file");
    }
    let output = Command::new("python3")
        .arg(field_dir().join("bench.py"))
        .args(["--job", "05-gpu-heat", "--arm", "python", "--score-only"])
        .arg(&workspace)
        .arg("--scratch")
        .arg(std::env::temp_dir().join(format!("miri-field-kernel-scratch-{}", case)))
        .output()
        .expect("cannot run python3");
    let _ = fs::remove_dir_all(&workspace);
    assert!(
        output.status.success(),
        "scoring failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_a_gpu_job_answered_on_the_processor_is_not_on_the_gpu() {
    if !available("python3") || !available("uv") {
        println!("skipping: python3 or uv is not on PATH, so the program was not scored");
        return;
    }
    let reference = field_dir()
        .join("jobs")
        .join("05-gpu-heat")
        .join("reference")
        .join("solution.py");
    let score = score_heat_workspace("cpu", &reference, &[]);
    assert!(
        score.contains("\"notOnGpu\": true")
            && score.contains("\"missing\": [\n      \"@compute\""),
        "a program with no kernel was not recorded as off the GPU:\n{}",
        score
    );
    let summed = score
        .split("\"passed\": ")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .unwrap_or("");
    assert_ne!(
        summed, "0",
        "the processor program should still pass the cases themselves"
    );
}

#[test]
fn test_a_gpu_job_with_a_shader_and_a_dispatch_is_on_the_gpu() {
    if !available("python3") || !available("uv") {
        println!("skipping: python3 or uv is not on PATH, so the program was not scored");
        return;
    }
    let reference = field_dir()
        .join("jobs")
        .join("05-gpu-heat")
        .join("reference")
        .join("solution.py");
    let score = score_heat_workspace(
        "gpu",
        &reference,
        &[
            ("stencil.wgsl", "@compute @workgroup_size(64) fn main() {}"),
            (
                "launch.py",
                "def run(pass_):\n    pass_.dispatch_workgroups(1)\n",
            ),
        ],
    );
    assert!(
        score.contains("\"notOnGpu\": false"),
        "a shader and its dispatch were not taken as evidence of a kernel:\n{}",
        score
    );
}

#[test]
fn test_every_arm_says_how_a_kernel_is_evidenced() {
    let path = field_dir().join("arms.toml");
    let parsed: toml::Value = toml::from_str(&read(&path))
        .unwrap_or_else(|e| panic!("cannot parse {}: {}", path.display(), e));
    let arms = parsed["arm"].as_array().expect("arms.toml lists no arms");
    for arm in arms {
        let id = arm["id"].as_str().unwrap_or("?");
        let evidence = arm.get("kernel_evidence").unwrap_or_else(|| {
            panic!(
                "arm {} does not say how a GPU job's kernel is evidenced",
                id
            )
        });
        let files = evidence["files"].as_array().map_or(0, Vec::len);
        let patterns = evidence["patterns"].as_array().map_or(0, Vec::len);
        assert!(
            files > 0 && patterns > 0,
            "arm {} names no files or no patterns for its kernel evidence",
            id
        );
    }
}

/// The GPU jobs written after the first round, each carrying a Miri solution
/// and a Python one on the GPU library beside its reference.
const WON_GPU_JOBS: [&str; 4] = [
    "06-gpu-life",
    "07-gpu-escape-time",
    "08-gpu-histogram",
    "09-gpu-particles",
];

/// Whether `source` carries every kernel pattern `arm` declares.
fn carries_kernel_evidence(arm: &str, source: &str) -> bool {
    let code = format!(
        "import re, tomllib\n\
         arms = tomllib.load(open({:?}, 'rb'))['arm']\n\
         patterns = next(a for a in arms if a['id'] == {:?})['kernel_evidence']['patterns']\n\
         text = open({:?}).read()\n\
         print(all(re.search(p, text) for p in patterns))",
        field_dir().join("arms.toml").display().to_string(),
        arm,
        source
    );
    let output = Command::new("python3")
        .arg("-c")
        .arg(code)
        .output()
        .expect("cannot run python3");
    String::from_utf8_lossy(&output.stdout).trim() == "True"
}

/// A GPU job is only worth measuring if it can be won. Each one ships a Miri
/// solution and a Python one written against the job's contract; both were run
/// on a GPU against the hidden cases when the job was written. Here, on any
/// machine, the Miri solution must still build with today's compiler, and both
/// must carry the kernel evidence their arm is scored on, so neither can drift
/// into an answer the scorer would refuse.
#[test]
fn test_every_gpu_job_carries_solutions_that_build_and_show_a_kernel() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the evidence was not read");
        return;
    }
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");
    for job in WON_GPU_JOBS {
        let reference = field_dir().join("jobs").join(job).join("reference");
        let miri_solution = reference.join("solution.mi");
        let python_solution = reference.join("solution_wgpu.py");
        assert!(
            carries_kernel_evidence("miri-pack", &miri_solution.display().to_string()),
            "{} would be scored as off the GPU",
            miri_solution.display()
        );
        assert!(
            carries_kernel_evidence("python", &python_solution.display().to_string()),
            "{} would be scored as off the GPU",
            python_solution.display()
        );
        let artifact = std::env::temp_dir().join(format!("miri-field-solution-{}", job));
        let built = crate::utils::miri_cmd()
            .env("MIRI_STDLIB_PATH", &stdlib)
            .arg("build")
            .arg(&miri_solution)
            .arg("-o")
            .arg(&artifact)
            .output()
            .expect("cannot run miri");
        let _ = fs::remove_file(&artifact);
        assert!(
            built.status.success(),
            "{} no longer builds:\n{}{}",
            miri_solution.display(),
            String::from_utf8_lossy(&built.stdout),
            String::from_utf8_lossy(&built.stderr)
        );
    }
}

/// The onboarding text an onboarded arm is taught from before its brief.
fn onboarding_text() -> String {
    read(&field_dir().join("onboarding").join("ONBOARDING.md"))
}

#[test]
fn test_the_onboarding_text_names_nothing_a_job_is_about() {
    let text = onboarding_text().to_lowercase();
    assert!(
        text.contains("{{material}}"),
        "ONBOARDING.md no longer substitutes the material it teaches from"
    );
    let job_words: BTreeSet<String> = JOBS
        .iter()
        .flat_map(|job| job.id.split('-').skip(1))
        .filter(|word| *word != "gpu")
        .map(str::to_string)
        .collect();
    for word in &job_words {
        assert!(
            !text.contains(word.as_str()),
            "ONBOARDING.md names '{}', which belongs to a job: the onboarding \
             must be the same lesson for every job",
            word
        );
    }
    for word in LANGUAGE_WORDS.iter().filter(|word| **word != "miri") {
        assert!(
            !text.contains(word),
            "ONBOARDING.md names '{}', another arm's language or toolchain",
            word
        );
    }
}

#[test]
fn test_every_onboarded_arm_names_a_material_the_phase_knows() {
    let materials: toml::Value = toml::from_str(&read(
        &field_dir().join("onboarding").join("materials.toml"),
    ))
    .expect("onboarding/materials.toml does not parse");
    let arms: toml::Value =
        toml::from_str(&read(&field_dir().join("arms.toml"))).expect("arms.toml does not parse");
    let mut onboarded = 0;
    for arm in arms["arm"].as_array().expect("arms.toml lists no arms") {
        let Some(names) = arm.get("onboarding").and_then(toml::Value::as_array) else {
            continue;
        };
        onboarded += 1;
        for name in names {
            let name = name.as_str().expect("a material is named by a string");
            let material = materials.get(name).unwrap_or_else(|| {
                panic!(
                    "arm {:?} is taught from unknown material {}",
                    arm["id"], name
                )
            });
            assert!(
                material.get("describes").is_some(),
                "material {} has no phrase for the onboarding text",
                name
            );
            if let Some(source) = material.get("stage").and_then(|stage| stage.get("from")) {
                let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(source.as_str().expect("a staged path is a string"));
                assert!(
                    source.is_file(),
                    "material {} stages {}, which is missing",
                    name,
                    source.display()
                );
            }
        }
    }
    assert_eq!(
        onboarded, 3,
        "the skill, grammar and both arms are the onboarded ones"
    );
}

/// A stand-in harness for the two-phase launch: it records every invocation's
/// arguments and what the workspace held, reports a session, and settles a
/// different cost for the onboarding phase than for the measured one.
#[cfg(unix)]
fn install_two_phase_harness(bin: &Path) {
    use std::os::unix::fs::PermissionsExt;
    install_fake_harness(bin, None);
    let init = r#"{"type":"system","subtype":"init","session_id":"sess-onboard","apiKeySource":"none","mcp_servers":[],"plugins":[],"skills":[]}"#;
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo 'fake harness 1'; exit 0; fi\n\
         printf '%s' \"$*\" | tr '\\n' ' ' >> invocations.txt; echo >> invocations.txt\n\
         case \"$*\" in\n\
         *--resume*) echo '{init}'; echo '{{\"type\":\"result\",\"session_id\":\"sess-onboard\",\"usage\":{{\"input_tokens\":7,\"output_tokens\":3}}}}' ;;\n\
         *) ls > onboarding-saw.txt; echo '{init}'; echo '{{\"type\":\"result\",\"session_id\":\"sess-onboard\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":10}}}}' ;;\n\
         esac\n",
        init = init
    );
    let path = bin.join("claude");
    fs::write(&path, script).expect("cannot write the two-phase harness");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("cannot make the two-phase harness executable");
}

#[test]
#[cfg(unix)]
fn test_an_onboarded_arm_is_taught_first_and_measured_from_its_brief() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the run was not launched");
        return;
    }
    let root = std::env::temp_dir().join("miri-field-onboarding");
    let _ = fs::remove_dir_all(&root);
    install_two_phase_harness(&root.join("bin"));
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ran = Command::new("python3")
        .arg(field_dir().join("bench.py"))
        .args([
            "--job",
            PROBE_JOB,
            "--arm",
            "miri-grammar",
            "--model",
            "claude-sonnet",
        ])
        .args(["--round", "synthetic", "--records-root"])
        .arg(root.join("runs"))
        .arg("--scratch")
        .arg(root.join("scratch"))
        .env("PATH", path)
        .output()
        .expect("cannot run python3");
    assert!(
        ran.status.success(),
        "the run failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let workspace = root
        .join("scratch")
        .join("synthetic")
        .join(PROBE_JOB)
        .join("miri-grammar")
        .join("claude-sonnet")
        .join("1");
    let invocations = read(&workspace.join("invocations.txt"));
    let lines: Vec<&str> = invocations.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "an onboarded run is two launches:\n{}",
        invocations
    );
    assert!(
        !lines[0].contains("--resume") && lines[0].contains("GRAMMAR.peg"),
        "the first launch is not the onboarding prompt naming its material:\n{}",
        lines[0]
    );
    assert!(
        lines[1].contains("--resume sess-onboard") && lines[1].contains("BRIEF.md"),
        "the measured launch does not resume the onboarding session:\n{}",
        lines[1]
    );
    // The resumed session's shell may still be wherever onboarding left it, so
    // the measured prompt names the workspace by its absolute path.
    let root_path = fs::canonicalize(&workspace).expect("the workspace exists");
    assert!(
        lines[1].contains(&format!("Your workspace is {}.", root_path.display())),
        "the measured prompt does not name the workspace root {}:\n{}",
        root_path.display(),
        lines[1]
    );
    let saw = read(&workspace.join("onboarding-saw.txt"));
    assert!(
        saw.contains("GRAMMAR.peg") && !saw.contains("BRIEF.md"),
        "the job was in the directory while the subject was being taught:\n{}",
        saw
    );
    let records = root
        .join("runs")
        .join("synthetic")
        .join(PROBE_JOB)
        .join("miri-grammar")
        .join("claude-sonnet");
    let record: serde_json::Value =
        serde_json::from_str(&read(&records.join("1.json"))).expect("the record is not JSON");
    assert_eq!(
        record["tokens"]["in"], 7,
        "the measured cost took in the onboarding phase"
    );
    assert_eq!(record["tokens"]["out"], 3);
    assert_eq!(
        record["onboarding"]["tokens"]["in"], 100,
        "the onboarding cost was not kept apart"
    );
    assert_eq!(record["onboarding"]["material"][0], "grammar");
    assert!(
        records.join("1.onboarding.jsonl").is_file(),
        "the onboarding transcript was not kept beside the measured one"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_a_miri_kernel_is_evidenced_by_a_gpu_fn_or_a_device_forall() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the evidence was not read");
        return;
    }
    let cases = [
        (
            "gpu_fn",
            "gpu fn step(src Array<i32, 4>)\n    return\n",
            true,
        ),
        (
            "device_forall",
            "gpu var a = Array<i32, 4>()\nforall i in 0..4\n    a[i] = i\n",
            true,
        ),
        ("gpu_forall", "gpu forall i in 0..4\n    out[i] = i\n", true),
        (
            "host_forall",
            "var a = Array<i32, 4>()\nforall i in 0..4\n    a[i] = i\n",
            false,
        ),
        ("buffer_only", "gpu var a = Array<i32, 4>()\n", false),
    ];
    for (name, source, on_gpu) in cases {
        let workspace = std::env::temp_dir().join(format!("miri-field-evidence-{}", name));
        let _ = fs::remove_dir_all(&workspace);
        fs::create_dir_all(&workspace).expect("cannot create a workspace");
        fs::write(workspace.join("main.mi"), source).expect("cannot write main.mi");
        let missing = with_bench(&format!(
            "print(bool(bench.kernel_evidence(__import__('pathlib').Path({:?}), bench.arm_named('miri-skill'))['missing']))",
            workspace.display().to_string()
        ));
        let _ = fs::remove_dir_all(&workspace);
        assert_eq!(
            missing == "False",
            on_gpu,
            "{}: a Miri source was judged {} the GPU",
            name,
            if on_gpu { "off" } else { "on" }
        );
    }
}

#[test]
fn test_a_stream_line_that_is_not_an_event_is_skipped() {
    // Round r2 lost two runs to a transcript line that decoded to a bare JSON
    // string: the parser took it for an event and crashed the cell.
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the parser was not run");
        return;
    }
    let parsed = with_bench(
        "print(bench.parse_transcript('\"stray\"\\n{\"type\":\"result\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}\\n42\\n')[0]['in'])",
    );
    assert_eq!(
        parsed, "2",
        "a stray line stopped the transcript from being read"
    );
}

#[test]
fn test_a_pruned_staged_stdlib_is_staged_again() {
    // The system's temporary-directory cleaner once deleted most of a staged
    // standard library during a round; staging must notice and copy it again.
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so staging was not run");
        return;
    }
    let scratch = std::env::temp_dir().join("miri-field-staging-prune");
    let _ = fs::remove_dir_all(&scratch);
    let bin = scratch.join("bin");
    fs::create_dir_all(&bin).expect("cannot create a fake miri directory");
    fs::write(bin.join("miri"), "#!/bin/sh\necho fake\n").expect("cannot write a fake miri");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin.join("miri"), fs::Permissions::from_mode(0o755))
            .expect("cannot make the fake miri executable");
    }
    let code = [
        "import os".to_string(),
        format!(
            "os.environ['PATH'] = {:?} + os.pathsep + os.environ['PATH']",
            bin.display().to_string()
        ),
        format!(
            "target = bench.stage_toolchain({:?})",
            scratch.display().to_string()
        ),
        "os.remove(target / 'lib/miri/stdlib/system/io.mi')".to_string(),
        "print(bench.staged_stdlib_is_whole(target))".to_string(),
        format!(
            "again = bench.stage_toolchain({:?})",
            scratch.display().to_string()
        ),
        "print(bench.staged_stdlib_is_whole(again))".to_string(),
    ]
    .join("\n");
    let printed = with_bench(&code);
    let _ = fs::remove_dir_all(&scratch);
    assert_eq!(printed, "False\nTrue", "a pruned staged stdlib was reused");
}

/// The fold publishes, per Miri arm and per run, what the subject did on the
/// way: the codes the compiler reported and how many carried a repair, the
/// repairs applied, the lookups made and the failed compile cycles by cause.
/// Pinned on the committed round r2, so the shape is checked against real
/// transcripts.
#[test]
fn test_the_fold_publishes_what_each_miri_run_did() {
    if !available("python3") {
        println!("skipping: python3 is not on PATH, so the round was not folded");
        return;
    }
    let out = std::env::temp_dir().join("miri-field-usage-summary.json");
    let folded = Command::new("python3")
        .arg(field_dir().join("report.py"))
        .args(["--round", "r2", "--out"])
        .arg(&out)
        .output()
        .expect("cannot run python3");
    assert!(
        folded.status.success(),
        "the fold failed:\n{}",
        String::from_utf8_lossy(&folded.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_str(&read(&out)).expect("the summary is not JSON");
    let usage = &summary["usage"];
    for arm in ["miri-skill", "miri-grammar", "miri-both"] {
        let of_arm = &usage["arms"][arm];
        assert_eq!(of_arm["runs"], 45, "{arm} folded the wrong number of runs");
        for key in [
            "compiles",
            "fixApplied",
            "repairRatio",
            "diagnostics",
            "repairedDiagnostics",
            "lookups",
            "failedCycles",
            "onboardingShare",
        ] {
            assert!(!of_arm[key].is_null(), "{arm} publishes no {key}");
        }
    }
    let runs = usage["runs"].as_array().expect("usage.runs is not a list");
    assert_eq!(runs.len(), 135, "every Miri run is listed once");
    assert!(runs.iter().all(|run| run["run"]
        .as_str()
        .is_some_and(|id| id.split('/').count() == 4)));
    assert!(
        usage["arms"]["miri-skill"]["diagnostics"]["MER_IMP_005"].as_u64() > Some(0),
        "the most frequent r2 code is no longer counted"
    );
}
