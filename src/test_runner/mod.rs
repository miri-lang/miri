// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Discovery and execution of `@test` functions in `.mi` files.
//!
//! `miri test` walks a directory, collects every function marked `@test`,
//! compiles each declaring file once with a synthesized dispatcher appended,
//! and then runs one subprocess per test. Process isolation is what keeps a
//! failing assertion — which terminates its process — from ending the run.

use serde::Serialize;
use std::path::Path;

mod discovery;
mod harness;
pub mod report;
mod runner;

pub use discovery::{RejectedFile, RejectionReason};

/// One `@test` function found in a source file.
///
/// `@ignore` and `@xfail` both require a reason argument, so the presence of a
/// reason *is* the marker — there is no separate boolean to fall out of step
/// with it.
#[derive(Debug, Clone, PartialEq)]
pub struct TestMarker {
    pub name: String,
    pub ignore_reason: Option<String>,
    pub xfail_reason: Option<String>,
}

impl TestMarker {
    pub fn is_ignored(&self) -> bool {
        self.ignore_reason.is_some()
    }

    pub fn is_xfail(&self) -> bool {
        self.xfail_reason.is_some()
    }
}

/// What happened to one test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Ran and exited cleanly.
    Passed,
    /// Ran and exited non-zero.
    Failed,
    /// Carried `@ignore`, so it was never spawned.
    Ignored,
    /// Carried `@xfail` and failed, which is what it documents.
    ExpectedFailure,
    /// Carried `@xfail` but passed: the pinned bug is fixed and the marker
    /// must go, so this fails the run.
    UnexpectedPass,
    /// Died on a signal rather than exiting.
    Crashed,
    /// The dispatcher rejected its own arguments — a fault in the runner, not
    /// in the test.
    RunnerFault,
}

impl Outcome {
    /// True when this outcome leaves the run green.
    pub fn is_success(self) -> bool {
        matches!(
            self,
            Outcome::Passed | Outcome::ExpectedFailure | Outcome::Ignored
        )
    }
}

/// Structured information about an assertion failure (read from sidecar file).
#[derive(Debug, Clone, PartialEq)]
pub struct AssertionFailure {
    pub code: String,
    pub kind: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub expression: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: Option<String>,
}

/// The result of one test, ready to report.
#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub path: String,
    pub name: String,
    pub outcome: Outcome,
    /// The ignore/xfail reason, the captured stderr, or the fault description.
    pub detail: Option<String>,
    /// Structured assertion failure information (if available).
    #[serde(skip)]
    pub failure: Option<AssertionFailure>,
}

/// Everything one `miri test` invocation produced.
#[derive(Debug, Serialize)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub results: Vec<TestResult>,
    /// Files that declare `@test` functions but cannot be run as test files.
    pub rejected_files: Vec<RejectedFile>,
}

impl TestSummary {
    /// Tally results into a summary.
    ///
    /// The rule for which outcomes count as passed, failed or ignored lives
    /// here and nowhere else, so a change to it cannot half-land.
    pub fn from_results(results: Vec<TestResult>, rejected: Vec<RejectedFile>) -> Self {
        let ignored = count_outcome(&results, Outcome::Ignored);
        let passed = results
            .iter()
            .filter(|r| r.outcome.is_success() && r.outcome != Outcome::Ignored)
            .count();
        let failed = results.iter().filter(|r| !r.outcome.is_success()).count();

        TestSummary {
            total: results.len(),
            passed,
            failed,
            ignored,
            results,
            rejected_files: rejected,
        }
    }

    /// True when nothing failed and every discovered file was runnable.
    ///
    /// A rejected file counts against the run: it holds tests that were never
    /// executed, and reporting that as success is the greenwash this runner
    /// exists to avoid.
    pub fn is_green(&self) -> bool {
        self.failed == 0 && self.rejected_files.is_empty()
    }
}

/// Discover and run every `@test` function `target` names.
///
/// `target` is either one file or a directory to walk. `filter` keeps only
/// tests whose `<path>::<name>` contains the substring; it narrows what either
/// form turned up rather than being how a single file is selected, so naming
/// `a.mi` cannot also run the `xa.mi` beside it.
///
/// Formatting is delegated to the caller.
pub fn run_tests(target: &Path, filter: Option<&str>) -> std::io::Result<TestSummary> {
    let discovered = discovery::discover(target)?;
    let root = discovery::root_of(target);
    let mut results = Vec::new();

    for file in discovered.files {
        let display = display_path(&file.path, &root);
        let selected = select_tests(&file.tests, &display, filter);
        if selected.is_empty() {
            continue;
        }
        results.extend(run_file(&file, &display, &selected));
    }

    let summary = TestSummary::from_results(results, discovered.rejected);
    Ok(summary)
}

/// The tests in one file that survive the filter, in declaration order.
fn select_tests<'a>(
    tests: &'a [TestMarker],
    display: &str,
    filter: Option<&str>,
) -> Vec<&'a TestMarker> {
    tests
        .iter()
        .filter(|test| match filter {
            Some(needle) => format!("{}::{}", display, test.name).contains(needle),
            None => true,
        })
        .collect()
}

/// Compile one file and run its selected tests, preserving declaration order.
fn run_file(
    file: &discovery::TestFile,
    display: &str,
    selected: &[&TestMarker],
) -> Vec<TestResult> {
    use tempfile::TempDir;

    let to_run: Vec<TestMarker> = selected
        .iter()
        .filter(|test| !test.is_ignored())
        .map(|test| (*test).clone())
        .collect();

    // Every selected test is ignored, so nothing needs compiling.
    if to_run.is_empty() {
        return selected
            .iter()
            .map(|test| ignored_result(test, display))
            .collect();
    }

    let artifact = match runner::compile_with_harness(&file.path, &file.source, &to_run) {
        Ok(artifact) => artifact,
        Err(error) => {
            return selected
                .iter()
                .map(|test| compile_failure_result(test, display, &error))
                .collect()
        }
    };

    // Create a temporary directory for sidecar files (or None if creation fails)
    let temp_dir = TempDir::new().ok();

    run_selected_tests(selected, display, artifact.executable(), temp_dir.as_ref())
}

/// Run selected tests with or without sidecar support.
/// If temp_dir is None, execution proceeds without sidecar files.
fn run_selected_tests(
    selected: &[&TestMarker],
    display: &str,
    executable: &std::path::Path,
    temp_dir: Option<&tempfile::TempDir>,
) -> Vec<TestResult> {
    selected
        .iter()
        .map(|test| {
            if test.is_ignored() {
                return ignored_result(test, display);
            }
            let sidecar_path = temp_dir.map(|d| d.path().join(format!("assert_{}", test.name)));
            let (execution, failure) =
                runner::execute_test(executable, &test.name, sidecar_path.as_deref());
            runner::to_test_result(display, test, execution, failure)
        })
        .collect()
}

fn ignored_result(test: &TestMarker, display: &str) -> TestResult {
    TestResult {
        path: display.to_string(),
        name: test.name.clone(),
        outcome: Outcome::Ignored,
        detail: test.ignore_reason.clone(),
        failure: None,
    }
}

/// A file that will not compile fails every test it declares, including the
/// ignored ones: the failure is the file's, and hiding it behind `ignored`
/// would report a broken file as a clean run.
fn compile_failure_result(test: &TestMarker, display: &str, error: &str) -> TestResult {
    TestResult {
        path: display.to_string(),
        name: test.name.clone(),
        outcome: Outcome::Failed,
        detail: Some(error.to_string()),
        failure: None,
    }
}

fn count_outcome(results: &[TestResult], outcome: Outcome) -> usize {
    results.iter().filter(|r| r.outcome == outcome).count()
}

/// The path as the report shows it: relative to the searched directory when it
/// sits underneath it, so output stays readable from a project root.
fn display_path(path: &Path, dir: &Path) -> String {
    path.strip_prefix(dir)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(name: &str) -> TestMarker {
        TestMarker {
            name: name.to_string(),
            ignore_reason: None,
            xfail_reason: None,
        }
    }

    fn result(name: &str, outcome: Outcome) -> TestResult {
        TestResult {
            path: "a.mi".to_string(),
            name: name.to_string(),
            outcome,
            detail: None,
            failure: None,
        }
    }

    #[test]
    fn expected_failure_counts_as_passing() {
        let summary = TestSummary::from_results(
            vec![
                result("a", Outcome::Passed),
                result("b", Outcome::ExpectedFailure),
            ],
            Vec::new(),
        );
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 0);
        assert!(summary.is_green());
    }

    #[test]
    fn unexpected_pass_fails_the_run() {
        let summary =
            TestSummary::from_results(vec![result("a", Outcome::UnexpectedPass)], Vec::new());
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.passed, 0);
        assert!(!summary.is_green());
    }

    #[test]
    fn ignored_is_counted_apart_from_passed() {
        let summary = TestSummary::from_results(vec![result("a", Outcome::Ignored)], Vec::new());
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.passed, 0);
        assert!(summary.is_green());
    }

    #[test]
    fn a_rejected_file_keeps_the_run_red() {
        let summary = TestSummary::from_results(
            vec![result("a", Outcome::Passed)],
            vec![RejectedFile {
                path: "bad.mi".to_string(),
                reason: RejectionReason::DeclaresMain,
            }],
        );
        assert_eq!(summary.failed, 0);
        assert!(!summary.is_green());
    }

    #[test]
    fn filter_matches_on_path_and_test_name() {
        let tests = vec![marker("test_adds"), marker("test_divides")];
        let selected = select_tests(&tests, "math.mi", Some("divides"));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "test_divides");

        let by_path = select_tests(&tests, "math.mi", Some("math.mi"));
        assert_eq!(by_path.len(), 2);
    }
}
