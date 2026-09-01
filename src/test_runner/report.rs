// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Rendering of a finished run, in the shape `cargo test` reports.

use crate::test_runner::{Outcome, RejectedFile, TestResult, TestSummary};

/// Render a run for a terminal.
pub fn format_pretty(summary: &TestSummary) -> String {
    let mut output = format!("running {} tests\n", summary.total);

    for result in &summary.results {
        output.push_str(&format!(
            "test {}::{} ... {}\n",
            result.path,
            result.name,
            status_line(result)
        ));
    }

    output.push_str(&failure_details(summary));
    output.push_str(&rejection_details(&summary.rejected_files));
    output.push_str(&result_line(summary));
    output
}

/// The trailing verdict on one test's line.
fn status_line(result: &TestResult) -> String {
    match result.outcome {
        Outcome::Passed => "ok".to_string(),
        Outcome::Failed => "FAILED".to_string(),
        Outcome::Ignored => match &result.detail {
            Some(reason) => format!("ignored, {}", reason),
            None => "ignored".to_string(),
        },
        Outcome::ExpectedFailure => "ok (expected failure)".to_string(),
        Outcome::UnexpectedPass => "FAILED (unexpected pass)".to_string(),
        Outcome::Crashed => "FAILED (crashed)".to_string(),
        Outcome::RunnerFault => "FAILED (runner error)".to_string(),
    }
}

/// The `failures:` block, omitted entirely when nothing failed.
fn failure_details(summary: &TestSummary) -> String {
    let mut failures = summary
        .results
        .iter()
        .filter(|result| !result.outcome.is_success())
        .peekable();

    if failures.peek().is_none() {
        return String::new();
    }

    let mut output = String::from("\nfailures:\n\n");
    for failure in failures {
        output.push_str(&format!("---- {}::{} ----\n", failure.path, failure.name));
        if let Some(detail) = &failure.detail {
            output.push_str(detail.trim_end());
            output.push('\n');
        }
        output.push('\n');
    }
    output
}

/// Files that hold tests but could not be run. Listed separately from failures
/// because nothing in them executed at all.
fn rejection_details(rejected: &[RejectedFile]) -> String {
    if rejected.is_empty() {
        return String::new();
    }

    let mut output = String::from("\nnot run:\n\n");
    for file in rejected {
        output.push_str(&format!("---- {} ----\n{}\n\n", file.path, file.reason));
    }
    output
}

fn result_line(summary: &TestSummary) -> String {
    let verdict = if summary.is_green() { "ok" } else { "FAILED" };
    let mut line = format!(
        "test result: {}. {} passed; {} failed; {} ignored",
        verdict, summary.passed, summary.failed, summary.ignored
    );
    if !summary.rejected_files.is_empty() {
        line.push_str(&format!(
            "; {} file(s) not run",
            summary.rejected_files.len()
        ));
    }
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::RejectionReason;

    fn result(name: &str, outcome: Outcome, detail: Option<&str>) -> TestResult {
        TestResult {
            path: "math.mi".to_string(),
            name: name.to_string(),
            outcome,
            detail: detail.map(str::to_string),
            failure: None,
        }
    }

    fn summary_of(results: Vec<TestResult>, rejected: Vec<RejectedFile>) -> TestSummary {
        TestSummary::from_results(results, rejected)
    }

    #[test]
    fn an_expected_failure_reads_as_ok_not_failed() {
        let rendered = format_pretty(&summary_of(
            vec![result(
                "test_bug",
                Outcome::ExpectedFailure,
                Some("known bug"),
            )],
            Vec::new(),
        ));
        assert!(rendered.contains("test math.mi::test_bug ... ok (expected failure)"));
        assert!(rendered.contains("test result: ok."));
        assert!(!rendered.contains("FAILED"));
    }

    #[test]
    fn an_unexpected_pass_reads_as_failed() {
        let rendered = format_pretty(&summary_of(
            vec![result(
                "test_bug",
                Outcome::UnexpectedPass,
                Some("remove it"),
            )],
            Vec::new(),
        ));
        assert!(rendered.contains("FAILED (unexpected pass)"));
        assert!(rendered.contains("test result: FAILED."));
    }

    #[test]
    fn the_failures_block_is_absent_when_nothing_failed() {
        let rendered = format_pretty(&summary_of(
            vec![result("test_a", Outcome::Passed, None)],
            Vec::new(),
        ));
        assert!(!rendered.contains("failures:"));
        assert!(rendered.contains("test result: ok. 1 passed; 0 failed; 0 ignored"));
    }

    #[test]
    fn an_empty_run_is_green_and_quiet() {
        let rendered = format_pretty(&summary_of(Vec::new(), Vec::new()));
        assert!(rendered.starts_with("running 0 tests\n"));
        assert!(!rendered.contains("failures:"));
        assert!(rendered.contains("test result: ok."));
    }

    #[test]
    fn an_ignored_test_shows_its_reason() {
        let rendered = format_pretty(&summary_of(
            vec![result("test_x", Outcome::Ignored, Some("flaky on CI"))],
            Vec::new(),
        ));
        assert!(rendered.contains("test math.mi::test_x ... ignored, flaky on CI"));
    }

    #[test]
    fn a_failure_shows_its_captured_stderr() {
        let rendered = format_pretty(&summary_of(
            vec![result(
                "test_x",
                Outcome::Failed,
                Some("Runtime error: assertion failed at math.mi:4"),
            )],
            Vec::new(),
        ));
        assert!(rendered.contains("---- math.mi::test_x ----"));
        assert!(rendered.contains("assertion failed at math.mi:4"));
    }

    #[test]
    fn a_rejected_file_is_listed_and_turns_the_run_red() {
        let rendered = format_pretty(&summary_of(
            Vec::new(),
            vec![RejectedFile {
                path: "bad.mi".to_string(),
                reason: RejectionReason::DeclaresMain,
            }],
        ));
        assert!(rendered.contains("not run:"));
        assert!(rendered.contains("---- bad.mi ----"));
        assert!(rendered.contains("test result: FAILED."));
        assert!(rendered.contains("1 file(s) not run"));
    }
}
