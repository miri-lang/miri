// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Agent-loop evaluation harness.
//!
//! The agent contract — stable diagnostic codes, a JSON envelope, `explain`,
//! `fix`, `view`, `patch` — is only worth its cost if it measurably shortens
//! tool-driven work. This is the measuring device.
//!
//! Each task under `evals/<id>/` is a recorded transcript: the ordered sequence
//! of invocations a tool would issue to finish one job, stored as data in
//! `steps.toml`, starting from the files in `seed/`. Replaying a transcript
//! records what the loop cost. What is replayed is the *agent's decisions*; the
//! compiler is the real binary and its output is never recorded or mocked, so a
//! change in the compiler moves the numbers.
//!
//! Metrics are measured from the outside — from what the harness observes on
//! stdout and on disk — and never read from a field the compiler populates
//! about itself. A measuring device must not ask its subject for its own score:
//! were the number self-reported, a regression that stopped reporting it would
//! read as an improvement.
//!
//! Byte counts are taken over *normalized* output. Two things in the envelope
//! vary between identical runs — `durationMs`, and absolute paths carrying the
//! temporary directory's name — and counting them raw would make the committed
//! baseline unreproducible. See [`normalize_output`].
//!
//! Wall-clock is printed to stdout and deliberately never committed: it measures
//! the load on the machine that happened to run the suite, not the cost of the
//! loop.

use crate::utils::miri_cmd;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tempfile::TempDir;

/// The tasks in the replay set, in the order the results table lists them.
///
/// The list is explicit rather than discovered by reading the directory: a
/// fixture that goes missing must fail the run, not silently shrink the corpus.
const TASKS: &[(&str, &str)] = &[
    ("a", "build hello world from an empty directory"),
    ("b", "repair a broken program using check, explain and fix"),
    ("c", "add a function and its test"),
    ("d", "extend a program with a stdlib module"),
    ("e", "recover from a capability rejection"),
    ("f", "make a failing test pass"),
];

/// One recorded step, as written in `steps.toml`.
///
/// `deny_unknown_fields` is load-bearing: a mistyped assertion key must fail the
/// run loudly. Silently ignoring it would leave a fixture that asserts nothing
/// while still reporting success, which is the failure mode this whole harness
/// exists to detect.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepSpec {
    #[serde(rename = "type")]
    kind: String,
    file: Option<String>,
    code: Option<String>,
    name: Option<String>,
    fn_name: Option<String>,
    old_text: Option<String>,
    new_text: Option<String>,
    body: Option<String>,
    /// The declarations an `InsertFn` step adds, in the order it names them.
    #[serde(default)]
    insert: Vec<InsertSpec>,
    path: Option<String>,
    content: Option<String>,
    dir: Option<String>,
    #[serde(default)]
    format_json: bool,
    #[serde(default = "yes")]
    must_succeed: bool,
    assert_diagnostic_code: Option<String>,
    #[serde(default)]
    assert_output_contains: Vec<String>,
    #[serde(default)]
    assert_file_changed: Vec<String>,
}

fn yes() -> bool {
    true
}

/// One declaration an `InsertFn` step adds.
///
/// A step carries a list of these rather than a single declaration because one
/// `miri patch` call can add several, and a transcript that spent one call per
/// declaration would report a cost the loop does not actually pay.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InsertSpec {
    /// The name being created: a bare name, or `Class.method`.
    name: String,
    /// The declaration's source text.
    body: String,
    /// The declaration this one follows; appended when absent.
    after: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscriptFile {
    #[serde(default)]
    step: Vec<StepSpec>,
}

/// A loaded transcript: the task it belongs to and the steps to replay.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub id: String,
    pub description: String,
    steps: Vec<StepSpec>,
}

/// The gated metrics for one task.
///
/// This type deliberately has no timing field. Wall-clock is carried separately
/// and never committed, so it cannot reach the baseline by someone adding it
/// here later without noticing what that costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// Task identifier, matching its directory under `evals/`.
    pub task: String,
    /// Whether every step's assertions held.
    pub success: bool,
    /// Compiler invocations the transcript made. Writing a file is not one.
    pub invocations: usize,
    /// Bytes of normalized stdout and stderr the loop had to ingest.
    pub bytes_read: usize,
    /// Bytes of `.mi` source the loop caused to be written, whether the writer
    /// was the agent or the compiler.
    pub bytes_written: usize,
}

/// Replace the parts of the compiler's output that differ between identical
/// runs, so a byte count taken over it is reproducible.
///
/// `durationMs` carries how long the run took. Absolute paths carry the name of
/// the temporary directory the task ran in, whose length varies by platform and
/// by run. Neither says anything about the cost of the loop, and both would make
/// the committed baseline change on every run.
fn normalize_output(output: &str, work_dir: &Path) -> String {
    // Compiled once. Both patterns are known good — this one is a literal, and
    // the path patterns below come from `regex::escape`, which always compiles —
    // so a failure here is a bug in the harness, not a condition to absorb.
    // Skipping normalization silently would leave the byte count carrying the
    // very fields it exists to remove, while still reporting a number.
    static DURATION: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#""durationMs":\s*\d+"#).expect("the durationMs pattern must compile")
    });

    let mut result = DURATION
        .replace_all(output, r#""durationMs":0"#)
        .into_owned();

    // Rewrite the working directory to a fixed stand-in. The canonical form is
    // rewritten too: macOS resolves temporary directories through /private, so
    // the compiler may report a path the harness never constructed.
    let mut roots: Vec<PathBuf> = vec![work_dir.to_path_buf()];
    if let Ok(canonical) = work_dir.canonicalize() {
        if canonical != work_dir {
            roots.push(canonical);
        }
    }

    normalize_paths(&result, &roots)
}

/// Rewrite every occurrence of `roots` in `text` to a fixed stand-in.
///
/// The longest root is rewritten first. One root can contain another — macOS
/// canonicalizes `/var/...` to `/private/var/...` — and rewriting the shorter
/// one first would consume its tail and strand the `/private` prefix, leaving
/// the byte count carrying a platform-dependent eight characters per path.
fn normalize_paths(text: &str, roots: &[PathBuf]) -> String {
    let mut ordered: Vec<&PathBuf> = roots.iter().collect();
    ordered.sort_by_key(|root| std::cmp::Reverse(root.as_os_str().len()));

    let mut result = text.to_string();
    for root in ordered {
        if let Some(text) = root.to_str() {
            let re = regex::Regex::new(&regex::escape(text))
                .expect("an escaped literal path must compile as a pattern");
            result = re.replace_all(&result, ".").into_owned();
        }
    }
    result
}

/// A snapshot of every `.mi` file in the working directory.
///
/// Only source is tracked. `run` and `build` drop executables beside it, whose
/// size is neither stable nor part of what an editing loop pays for.
fn snapshot_sources(work_dir: &Path) -> BTreeMap<String, String> {
    let mut snapshot = BTreeMap::new();
    let entries = match fs::read_dir(work_dir) {
        Ok(entries) => entries,
        Err(_) => return snapshot,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mi") {
            continue;
        }
        if let (Some(name), Ok(content)) = (
            path.file_name().and_then(|n| n.to_str()),
            fs::read_to_string(&path),
        ) {
            snapshot.insert(name.to_string(), content);
        }
    }
    snapshot
}

/// Bytes of source written between two snapshots: for every file that is new or
/// whose content changed, the size of what now stands on disk.
fn bytes_written_between(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> usize {
    after
        .iter()
        .filter(|(name, content)| before.get(*name) != Some(content))
        .map(|(_, content)| content.len())
        .sum()
}

/// Report a step that does not carry the field its kind needs.
fn require<'a>(value: &'a Option<String>, field: &str, kind: &str) -> Result<&'a str, String> {
    value
        .as_deref()
        .ok_or_else(|| format!("step of type '{}' requires field '{}'", kind, field))
}

/// What running one step produced.
struct StepOutcome {
    succeeded: bool,
    output: String,
    /// Bytes of normalized output, counted only for compiler invocations.
    bytes_read: usize,
    /// Whether this step invoked the compiler.
    invoked_compiler: bool,
}

/// Run one step against the real binary.
fn run_step(
    spec: &StepSpec,
    work_dir: &Path,
    stdlib_path: &Path,
    scratch: &Path,
) -> Result<StepOutcome, String> {
    // Writing a file is the agent's own work, not a compiler invocation.
    if spec.kind == "WriteFile" {
        let path = require(&spec.path, "path", &spec.kind)?;
        let content = require(&spec.content, "content", &spec.kind)?;
        fs::write(work_dir.join(path), content).map_err(|e| e.to_string())?;
        return Ok(StepOutcome {
            succeeded: true,
            output: String::new(),
            bytes_read: 0,
            invoked_compiler: false,
        });
    }

    let mut cmd = miri_cmd();
    cmd.env("MIRI_STDLIB_PATH", stdlib_path)
        .env("RUST_BACKTRACE", "0")
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .current_dir(work_dir);

    match spec.kind.as_str() {
        "Check" => {
            cmd.arg("check")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--format")
                .arg("json");
        }
        "Explain" => {
            cmd.arg("explain")
                .arg(require(&spec.code, "code", &spec.kind)?);
            if spec.format_json {
                cmd.arg("--format").arg("json");
            }
        }
        "FixPlan" => {
            cmd.arg("fix")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--plan")
                .arg("--format")
                .arg("json");
        }
        "FixApply" => {
            cmd.arg("fix")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--apply")
                .arg("--yes")
                .arg("--format")
                .arg("json");
        }
        "ViewFn" => {
            cmd.arg("view")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--fn")
                .arg(require(&spec.name, "name", &spec.kind)?);
            if spec.format_json {
                cmd.arg("--format").arg("json");
            }
        }
        "ViewOutline" => {
            cmd.arg("view")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--outline");
            if spec.format_json {
                cmd.arg("--format").arg("json");
            }
        }
        "Patch" => {
            cmd.arg("patch")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--replace-in-fn")
                .arg(require(&spec.fn_name, "fn_name", &spec.kind)?)
                .arg("--old")
                .arg(require(&spec.old_text, "old_text", &spec.kind)?)
                .arg("--new")
                .arg(require(&spec.new_text, "new_text", &spec.kind)?)
                .arg("--format")
                .arg("json");
        }
        "ReplaceFn" => {
            // The body travels in a file so a multi-line replacement needs no
            // shell quoting. It is written outside the working directory:
            // anything left beside the source would be picked up by a later
            // `miri test --dir .` in the same transcript.
            let body = require(&spec.body, "body", &spec.kind)?;
            let body_path = scratch.join("body.txt");
            fs::write(&body_path, body).map_err(|e| e.to_string())?;
            cmd.arg("patch")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--replace-fn")
                .arg(require(&spec.fn_name, "fn_name", &spec.kind)?)
                .arg("--body-file")
                .arg(&body_path)
                .arg("--format")
                .arg("json");
        }
        "InsertFn" => {
            if spec.insert.is_empty() {
                return Err(format!(
                    "step of type '{}' requires field 'insert'",
                    spec.kind
                ));
            }
            cmd.arg("patch")
                .arg(require(&spec.file, "file", &spec.kind)?);
            // Each declaration travels in its own file, written outside the
            // working directory: anything left beside the source would be
            // picked up by a later `miri test --dir .` in the same transcript.
            for (index, insert) in spec.insert.iter().enumerate() {
                let body_path = scratch.join(format!("insert-{}.txt", index));
                fs::write(&body_path, &insert.body).map_err(|e| e.to_string())?;
                cmd.arg("--insert-fn").arg(&insert.name);
                cmd.arg("--body-file").arg(&body_path);
                if let Some(after) = &insert.after {
                    cmd.arg("--after").arg(after);
                }
            }
            cmd.arg("--format").arg("json");
        }
        "Run" => {
            cmd.arg("run").arg(require(&spec.file, "file", &spec.kind)?);
        }
        "TestDir" => {
            cmd.arg("test")
                .arg("--dir")
                .arg(require(&spec.dir, "dir", &spec.kind)?);
        }
        "Build" => {
            cmd.arg("build")
                .arg(require(&spec.file, "file", &spec.kind)?)
                .arg("--format")
                .arg("json");
        }
        other => return Err(format!("unknown step type '{}'", other)),
    }

    let output = cmd.output().map_err(|e| e.to_string())?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes_read = normalize_output(&combined, work_dir).len();

    Ok(StepOutcome {
        succeeded: output.status.success(),
        output: combined,
        bytes_read,
        invoked_compiler: true,
    })
}

/// Check every assertion a step declared.
///
/// Both directions of `must_succeed` are enforced. A step recorded as failing
/// that starts succeeding changes the contract as surely as the reverse, and a
/// transcript whose "broken" program quietly began to compile would otherwise
/// keep passing while measuring nothing.
fn verify_step(
    spec: &StepSpec,
    outcome: &StepOutcome,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<(), String> {
    if spec.must_succeed && !outcome.succeeded {
        return Err(format!(
            "expected the step to succeed, but it failed. Output:\n{}",
            outcome.output
        ));
    }
    if !spec.must_succeed && outcome.succeeded {
        return Err(format!(
            "expected the step to fail, but it succeeded. Output:\n{}",
            outcome.output
        ));
    }

    for needle in &spec.assert_output_contains {
        if !outcome.output.contains(needle) {
            return Err(format!(
                "expected the output to contain {:?}. Output:\n{}",
                needle, outcome.output
            ));
        }
    }

    if let Some(code) = &spec.assert_diagnostic_code {
        // A step asserting a code must produce an envelope carrying it. Failing
        // to parse is a failure, never a reason to skip the check.
        let envelope: serde_json::Value = serde_json::from_str(&outcome.output).map_err(|e| {
            format!(
                "expected diagnostic {} in a JSON envelope, but the output did not parse ({}). Output:\n{}",
                code, e, outcome.output
            )
        })?;
        let diagnostics = envelope["diagnostics"].as_array().ok_or_else(|| {
            format!(
                "expected diagnostic {}, but the envelope carried no diagnostics array",
                code
            )
        })?;
        let found = diagnostics
            .iter()
            .any(|d| d["code"].as_str() == Some(code.as_str()));
        if !found {
            let seen: Vec<&str> = diagnostics
                .iter()
                .filter_map(|d| d["code"].as_str())
                .collect();
            return Err(format!(
                "expected diagnostic {}, but the envelope carried {:?}",
                code, seen
            ));
        }
    }

    for name in &spec.assert_file_changed {
        if after.get(name) == before.get(name) {
            return Err(format!(
                "expected {} to change, but its content is unchanged",
                name
            ));
        }
    }

    Ok(())
}

/// Replay one transcript in a prepared working directory.
///
/// Returns the gated metrics and, separately, the wall-clock the run took.
pub fn replay(
    transcript: &Transcript,
    work_dir: &Path,
    stdlib_path: &Path,
    scratch: &Path,
) -> Result<(TaskMetrics, u64), String> {
    let mut metrics = TaskMetrics {
        task: transcript.id.clone(),
        success: true,
        invocations: 0,
        bytes_read: 0,
        bytes_written: 0,
    };
    let started = Instant::now();

    for (index, spec) in transcript.steps.iter().enumerate() {
        let before = snapshot_sources(work_dir);
        let outcome = run_step(spec, work_dir, stdlib_path, scratch)
            .map_err(|e| format!("task {} step {}: {}", transcript.id, index + 1, e))?;
        let after = snapshot_sources(work_dir);

        if outcome.invoked_compiler {
            metrics.invocations += 1;
            metrics.bytes_read += outcome.bytes_read;
        }
        metrics.bytes_written += bytes_written_between(&before, &after);

        verify_step(spec, &outcome, &before, &after).map_err(|e| {
            format!(
                "task {} ({}) step {} [{}]: {}",
                transcript.id,
                transcript.description,
                index + 1,
                spec.kind,
                e
            )
        })?;
    }

    Ok((metrics, started.elapsed().as_millis() as u64))
}

fn evals_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals")
}

/// Load every transcript in the replay set.
pub fn load_transcripts() -> Result<Vec<Transcript>, String> {
    TASKS
        .iter()
        .map(|(id, description)| {
            let path = evals_dir().join(id).join("steps.toml");
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
            let parsed: TranscriptFile = toml::from_str(&text)
                .map_err(|e| format!("cannot parse {}: {}", path.display(), e))?;
            if parsed.step.is_empty() {
                return Err(format!("{} declares no steps", path.display()));
            }
            Ok(Transcript {
                id: (*id).to_string(),
                description: (*description).to_string(),
                steps: parsed.step,
            })
        })
        .collect()
}

/// Copy a task's seed into `work_dir`.
///
/// A task with no `seed/` directory starts from an empty one. That is not an
/// oversight: git cannot carry an empty directory, so the absence of the
/// directory is how "starts from nothing" is represented.
fn install_seed(id: &str, work_dir: &Path) -> Result<(), String> {
    let seed = evals_dir().join(id).join("seed");
    if !seed.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&seed).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        if from.is_file() {
            let name = from
                .file_name()
                .ok_or_else(|| format!("unnamed seed file in {}", seed.display()))?;
            fs::copy(&from, work_dir.join(name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Reject a transcript that would report success while proving nothing.
///
/// Every task must check what the compiler actually said — a diagnostic code or
/// a string in its output — and must either recover from a genuine failure or
/// change a file. A transcript that only runs commands and reads their exit
/// codes measures the loop's length without measuring whether it worked, and
/// would keep passing after the surface underneath it stopped working.
pub fn preflight(transcripts: &[Transcript]) -> Result<(), String> {
    for transcript in transcripts {
        let checks_content = transcript
            .steps
            .iter()
            .any(|s| s.assert_diagnostic_code.is_some() || !s.assert_output_contains.is_empty());
        if !checks_content {
            return Err(format!(
                "task {} ({}) asserts nothing about the compiler's output",
                transcript.id, transcript.description
            ));
        }

        let mutates = transcript
            .steps
            .iter()
            .any(|s| !s.assert_file_changed.is_empty());

        // A genuine recovery: a step recorded as failing, and a later one
        // recorded as succeeding. Order matters — the reverse is a regression,
        // not a repair.
        let recovers = match transcript.steps.iter().position(|s| !s.must_succeed) {
            Some(index) => transcript.steps[index + 1..].iter().any(|s| s.must_succeed),
            None => false,
        };

        if !mutates && !recovers {
            return Err(format!(
                "task {} ({}) is vacuous: it neither changes a file nor recovers from a failure",
                transcript.id, transcript.description
            ));
        }
    }
    Ok(())
}

/// Compare one task against its baseline, reporting what moved.
///
/// An improvement fails the gate exactly as a regression does. The committed
/// table records what the loop costs today, not a ceiling it must stay under: a
/// change that makes the loop cheaper should show up as a deliberate edit to
/// that record, in the same diff that earned it.
pub fn check_regression(baseline: &TaskMetrics, result: &TaskMetrics) -> Option<String> {
    let mut moved = Vec::new();
    if baseline.success != result.success {
        moved.push(format!(
            "success: {} -> {}",
            baseline.success, result.success
        ));
    }
    if baseline.invocations != result.invocations {
        moved.push(format!(
            "invocations: {} -> {}",
            baseline.invocations, result.invocations
        ));
    }
    if baseline.bytes_read != result.bytes_read {
        moved.push(format!(
            "bytesRead: {} -> {}",
            baseline.bytes_read, result.bytes_read
        ));
    }
    if baseline.bytes_written != result.bytes_written {
        moved.push(format!(
            "bytesWritten: {} -> {}",
            baseline.bytes_written, result.bytes_written
        ));
    }
    if moved.is_empty() {
        None
    } else {
        Some(format!("task {}: {}", result.task, moved.join(", ")))
    }
}

/// Compare a whole run against a whole baseline.
pub fn compare_all(baseline: &[TaskMetrics], results: &[TaskMetrics]) -> Result<(), String> {
    let mut problems = Vec::new();

    for result in results {
        match baseline.iter().find(|b| b.task == result.task) {
            Some(entry) => problems.extend(check_regression(entry, result)),
            None => problems.push(format!("task {}: not present in the baseline", result.task)),
        }
    }
    for entry in baseline {
        if !results.iter().any(|r| r.task == entry.task) {
            problems.push(format!("task {}: in the baseline but not run", entry.task));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Render the committed results table.
///
/// Wall-clock is absent by construction. Timing here would measure the load on
/// whichever machine ran the suite, and would rewrite this file on every run.
fn render_table(results: &[TaskMetrics]) -> String {
    let mut out = String::from(
        "# Agent-loop replay results\n\
         \n\
         What one tool-driven job costs against the current compiler. Every row is\n\
         a recorded transcript under `evals/<id>/`, replayed against the real\n\
         binary; the numbers are observed by the harness, not reported by the\n\
         compiler about itself.\n\
         \n\
         All four measured columns are gated: a run that does not reproduce them\n\
         fails. That includes a run that gets *cheaper* — a loop that improves\n\
         should update this table in the change that earned it, via\n\
         `make evals-bless`.\n\
         \n\
         Wall-clock is deliberately absent. It measures the load on the machine\n\
         that ran the suite rather than the cost of the loop, and it would rewrite\n\
         this file on every run. The harness prints it to stdout instead.\n\
         \n\
         | Task | What it does | Success | Invocations | Bytes read | Bytes written |\n\
         |------|--------------|---------|-------------|------------|---------------|\n",
    );
    for result in results {
        let description = TASKS
            .iter()
            .find(|(id, _)| *id == result.task)
            .map(|(_, d)| *d)
            .unwrap_or("");
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            result.task,
            description,
            if result.success { "yes" } else { "no" },
            result.invocations,
            result.bytes_read,
            result.bytes_written
        ));
    }
    out
}

fn baseline_json_path() -> PathBuf {
    evals_dir().join("results").join("baseline.json")
}

fn baseline_table_path() -> PathBuf {
    evals_dir().join("results").join("baseline.md")
}

/// Replay every task, in the order [`TASKS`] declares.
pub fn run_all() -> Result<Vec<TaskMetrics>, String> {
    let transcripts = load_transcripts()?;
    preflight(&transcripts)?;

    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let mut results = Vec::new();
    for transcript in &transcripts {
        let work = TempDir::new().map_err(|e| e.to_string())?;
        let scratch = TempDir::new().map_err(|e| e.to_string())?;
        install_seed(&transcript.id, work.path())?;
        let (metrics, wall_clock_ms) =
            replay(transcript, work.path(), &stdlib_path, scratch.path())?;
        println!(
            "eval {} ({}): {} invocations, {} bytes read, {} bytes written, {} ms",
            metrics.task,
            transcript.description,
            metrics.invocations,
            metrics.bytes_read,
            metrics.bytes_written,
            wall_clock_ms
        );
        results.push(metrics);
    }
    Ok(results)
}

#[test]
fn test_replay_matches_the_committed_baseline() {
    let results = run_all().expect("the replay set must run to completion");

    if std::env::var("MIRI_EVALS_BLESS").is_ok() {
        let json = serde_json::to_string_pretty(&results).expect("metrics must serialize") + "\n";
        fs::write(baseline_json_path(), json).expect("the baseline must be writable");
        fs::write(baseline_table_path(), render_table(&results))
            .expect("the table must be writable");
        return;
    }

    let committed = fs::read_to_string(baseline_json_path())
        .expect("evals/results/baseline.json must exist; regenerate it with make evals-bless");
    let baseline: Vec<TaskMetrics> =
        serde_json::from_str(&committed).expect("the baseline must parse");

    if let Err(diff) = compare_all(&baseline, &results) {
        panic!(
            "the agent loop no longer costs what the baseline records:\n{}\n\n\
             If the change is intended, re-record it with `make evals-bless` and \
             commit the updated table.",
            diff
        );
    }
}

#[test]
fn test_committed_table_matches_the_committed_metrics() {
    let committed = fs::read_to_string(baseline_json_path()).expect("the baseline must exist");
    let baseline: Vec<TaskMetrics> =
        serde_json::from_str(&committed).expect("the baseline must parse");
    let table = fs::read_to_string(baseline_table_path()).expect("the table must exist");

    assert_eq!(
        table,
        render_table(&baseline),
        "evals/results/baseline.md has drifted from baseline.json; run make evals-bless"
    );
}

#[test]
fn test_committed_baseline_carries_no_volatile_field() {
    // The table and the metrics are committed, so anything varying between
    // identical runs would dirty the working tree on every run and would
    // eventually be silenced by widening the gate.
    let json = fs::read_to_string(baseline_json_path()).expect("the baseline must exist");
    for volatile in ["wall_clock", "wallClock", "duration", "generated_at"] {
        assert!(
            !json.contains(volatile),
            "baseline.json carries the volatile field {:?}",
            volatile
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(task: &str, success: bool, invocations: usize) -> TaskMetrics {
        TaskMetrics {
            task: task.to_string(),
            success,
            invocations,
            bytes_read: 100,
            bytes_written: 10,
        }
    }

    fn metrics_with_bytes(task: &str, bytes_read: usize, bytes_written: usize) -> TaskMetrics {
        TaskMetrics {
            task: task.to_string(),
            success: true,
            invocations: 5,
            bytes_read,
            bytes_written,
        }
    }

    fn spec(kind: &str) -> StepSpec {
        StepSpec {
            kind: kind.to_string(),
            file: None,
            code: None,
            name: None,
            fn_name: None,
            old_text: None,
            new_text: None,
            body: None,
            insert: Vec::new(),
            path: None,
            content: None,
            dir: None,
            format_json: false,
            must_succeed: true,
            assert_diagnostic_code: None,
            assert_output_contains: Vec::new(),
            assert_file_changed: Vec::new(),
        }
    }

    #[test]
    fn test_normalize_output_elides_duration_and_paths() {
        let work = Path::new("/tmp/eval-xyz");
        let raw = r#"{"durationMs": 4172,"path":"/tmp/eval-xyz/program.mi"}"#;
        let normalized = normalize_output(raw, work);
        assert!(normalized.contains(r#""durationMs":0"#));
        assert!(normalized.contains(r#""path":"./program.mi""#));
    }

    #[test]
    fn test_a_root_nested_in_another_root_is_fully_rewritten() {
        // macOS resolves a temporary directory through /private, so the
        // harness holds /var/... while the compiler reports /private/var/...
        // The shorter root is a substring of the longer one; rewriting it
        // first would strip the tail and strand the /private prefix.
        let nested = PathBuf::from("/var/t/eval");
        let canonical = PathBuf::from("/private/var/t/eval");
        let raw = r#"{"path":"/private/var/t/eval/program.mi"}"#;

        for roots in [
            vec![nested.clone(), canonical.clone()],
            vec![canonical.clone(), nested.clone()],
        ] {
            assert_eq!(
                normalize_paths(raw, &roots),
                r#"{"path":"./program.mi"}"#,
                "the canonical root must be rewritten whichever order it arrives in"
            );
        }
    }

    #[test]
    fn test_normalized_byte_count_is_independent_of_the_temp_path() {
        let short = Path::new("/tmp/a");
        let long = Path::new("/tmp/a-considerably-longer-directory-name");
        let from_short = normalize_output(r#"{"path":"/tmp/a/p.mi","durationMs": 7}"#, short);
        let from_long = normalize_output(
            r#"{"path":"/tmp/a-considerably-longer-directory-name/p.mi","durationMs": 1234}"#,
            long,
        );
        assert_eq!(from_short.len(), from_long.len());
    }

    #[test]
    fn test_bytes_written_counts_only_changed_files() {
        let mut before = BTreeMap::new();
        before.insert("a.mi".to_string(), "one".to_string());
        before.insert("b.mi".to_string(), "unchanged".to_string());

        let mut after = before.clone();
        after.insert("a.mi".to_string(), "rewritten".to_string());
        after.insert("c.mi".to_string(), "new".to_string());

        // "rewritten" (9) + "new" (3); b.mi contributes nothing.
        assert_eq!(bytes_written_between(&before, &after), 12);
    }

    #[test]
    fn test_gate_is_quiet_when_nothing_moved() {
        let baseline = metrics("a", true, 3);
        assert_eq!(check_regression(&baseline, &baseline.clone()), None);
    }

    #[test]
    fn test_gate_fires_on_one_extra_invocation() {
        let baseline = metrics("b", true, 5);
        let degraded = metrics("b", true, 6);
        let report = check_regression(&baseline, &degraded).expect("the gate must fire");
        assert!(report.contains("task b"), "{}", report);
        assert!(report.contains("invocations: 5 -> 6"), "{}", report);
    }

    #[test]
    fn test_gate_fires_when_a_task_stops_working() {
        let baseline = metrics("c", true, 4);
        let broken = metrics("c", false, 4);
        let report = check_regression(&baseline, &broken).expect("the gate must fire");
        assert!(report.contains("success: true -> false"), "{}", report);
    }

    #[test]
    fn test_gate_fires_on_an_improvement_too() {
        let baseline = metrics("d", true, 6);
        let cheaper = metrics("d", true, 4);
        let report = check_regression(&baseline, &cheaper).expect("the gate must fire");
        assert!(report.contains("invocations: 6 -> 4"), "{}", report);
    }

    #[test]
    fn test_gate_fires_when_the_loop_reads_more() {
        // bytesRead and bytesWritten are gated, so each needs its own case. A
        // helper that pins them to one value leaves both branches untested and
        // a loop that doubled what it ingests would pass.
        let baseline = metrics_with_bytes("e", 1000, 10);
        let noisier = metrics_with_bytes("e", 1500, 10);
        let report = check_regression(&baseline, &noisier).expect("the gate must fire");
        assert!(report.contains("bytesRead: 1000 -> 1500"), "{}", report);
    }

    #[test]
    fn test_gate_fires_when_the_loop_writes_more() {
        let baseline = metrics_with_bytes("f", 100, 10);
        let heavier = metrics_with_bytes("f", 100, 64);
        let report = check_regression(&baseline, &heavier).expect("the gate must fire");
        assert!(report.contains("bytesWritten: 10 -> 64"), "{}", report);
    }

    #[test]
    fn test_compare_all_reports_a_task_that_vanished() {
        let baseline = vec![metrics("a", true, 1), metrics("b", true, 2)];
        let results = vec![metrics("a", true, 1)];
        let report = compare_all(&baseline, &results).expect_err("the gate must fire");
        assert!(report.contains("task b"), "{}", report);
    }

    #[test]
    fn test_compare_all_reports_a_task_with_no_baseline() {
        // The reverse of a vanished task: something ran that the committed
        // record says nothing about, so there is no cost to compare it against.
        let baseline = vec![metrics("a", true, 1)];
        let results = vec![metrics("a", true, 1), metrics("b", true, 2)];
        let report = compare_all(&baseline, &results).expect_err("the gate must fire");
        assert!(report.contains("not present in the baseline"), "{}", report);
    }

    #[test]
    fn test_a_transcript_with_no_steps_is_rejected() {
        // An empty transcript would report a task costing nothing and passing.
        let parsed: TranscriptFile = toml::from_str("").expect("an empty document is valid TOML");
        assert!(
            parsed.step.is_empty(),
            "the loader's empty-transcript guard must remain reachable"
        );
    }

    #[test]
    fn test_preflight_accepts_the_committed_corpus() {
        // The guard has to hold on the fixtures that actually ship. Exercising
        // it only against synthetic transcripts is how it ends up inert.
        let transcripts = load_transcripts().expect("the corpus must load");
        preflight(&transcripts).expect("every committed task must be non-vacuous");
    }

    #[test]
    fn test_preflight_rejects_a_transcript_that_asserts_nothing() {
        let transcript = Transcript {
            id: "x".to_string(),
            description: "asserts nothing".to_string(),
            steps: vec![spec("Check")],
        };
        let report = preflight(&[transcript]).expect_err("the guard must fire");
        assert!(report.contains("asserts nothing"), "{}", report);
    }

    #[test]
    fn test_preflight_rejects_a_transcript_that_neither_edits_nor_recovers() {
        let mut step = spec("Check");
        step.assert_output_contains = vec!["something".to_string()];
        let transcript = Transcript {
            id: "y".to_string(),
            description: "only reads".to_string(),
            steps: vec![step],
        };
        let report = preflight(&[transcript]).expect_err("the guard must fire");
        assert!(report.contains("vacuous"), "{}", report);
    }

    #[test]
    fn test_preflight_does_not_mistake_a_regression_for_a_recovery() {
        // Passing and then failing is the opposite of a repair. A guard that
        // treats the first failing step as evidence a recovery happened would
        // accept this transcript.
        let mut passing = spec("Check");
        passing.assert_output_contains = vec!["ok".to_string()];
        let mut failing = spec("Check");
        failing.must_succeed = false;

        let transcript = Transcript {
            id: "z".to_string(),
            description: "degrades".to_string(),
            steps: vec![passing, failing],
        };
        let report = preflight(&[transcript]).expect_err("the guard must fire");
        assert!(report.contains("vacuous"), "{}", report);
    }

    #[test]
    fn test_an_unknown_field_in_a_transcript_is_rejected() {
        // A mistyped assertion key must fail the run. Ignoring it would leave a
        // fixture that asserts nothing and still reports success.
        let text = r#"
            [[step]]
            type = "Check"
            file = "program.mi"
            assert_diagnostic_codes = "MER_TYP_042"
        "#;
        toml::from_str::<TranscriptFile>(text)
            .expect_err("an unknown field must be rejected at load");
    }

    #[test]
    fn test_a_corrupted_seed_fails_the_replay() {
        // The fixtures under evals/<id>/seed/ must be what the transcript runs
        // against. Were they dead weight, corrupting one would change nothing
        // and the harness would be measuring text it carries itself. That was
        // once true here, so this is checked for every seeded task rather than
        // for one: a task whose steps never read its seed would slip through a
        // single-task version of this test.
        let transcripts = load_transcripts().expect("the corpus must load");
        let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("stdlib");

        let mut checked = 0;
        for transcript in &transcripts {
            // Keyed on seed *files*, not the directory: an empty seed
            // directory would otherwise be corrupted to nothing and the task
            // would replay clean, counting as covered while proving nothing.
            let work = TempDir::new().expect("a working directory");
            install_seed(&transcript.id, work.path()).expect("the seed must install");
            if snapshot_sources(work.path()).is_empty() {
                continue;
            }

            let scratch = TempDir::new().expect("a scratch directory");
            for entry in fs::read_dir(work.path()).expect("the working directory must be readable")
            {
                let path = entry.expect("a readable entry").path();
                fs::write(&path, "not miri source @@@\n").expect("the seed copy must be writable");
            }

            let outcome = replay(transcript, work.path(), &stdlib_path, scratch.path());
            assert!(
                outcome.is_err(),
                "task {} replayed successfully against a corrupted seed, so its seed is not load-bearing",
                transcript.id
            );
            checked += 1;
        }

        assert!(
            checked >= 5,
            "expected every seeded task to be covered, but only {} were",
            checked
        );
    }
}
