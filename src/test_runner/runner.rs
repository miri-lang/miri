// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Compiling a test file and running its tests in isolated subprocesses.
//!
//! One compile per file, one spawn per test. A failing assertion terminates
//! its own process and nothing else, which is what lets the rest of the run
//! continue.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::codegen::{BuildTarget, CpuBackend};
use crate::pipeline::{BuildOptions, Pipeline};
use crate::test_runner::{harness, Outcome, TestMarker, TestResult};

/// How one test process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    /// Exited zero.
    Succeeded,
    /// Exited non-zero, carrying whatever it wrote to stderr.
    Errored(String),
    /// Died on a signal instead of exiting.
    Killed(i32),
    /// The dispatcher rejected its arguments, so no test ran.
    Fault(String),
}

/// A compiled test binary and the directory holding it.
///
/// The directory is owned so it outlives every spawn and is removed when the
/// file's tests are done.
pub struct Artifact {
    _directory: tempfile::TempDir,
    executable: PathBuf,
}

impl Artifact {
    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

/// Compile `source` plus a synthesized dispatcher into a throwaway binary.
///
/// The dispatcher is appended, never spliced, so every span in the user's own
/// source keeps pointing where it did and compile errors stay truthful.
pub fn compile_with_harness(
    file_path: &Path,
    source: &str,
    tests: &[TestMarker],
) -> Result<Artifact, String> {
    let combined = format!("{}\n{}", source, harness::synthesize(tests));

    let directory = tempfile::tempdir()
        .map_err(|error| format!("could not create a temporary directory: {}", error))?;
    let executable = directory.path().join("test_binary");

    let options = BuildOptions {
        out_path: Some(executable.clone()),
        release: false,
        opt_level: 0,
        cpu_backend: CpuBackend::Cranelift,
        target: BuildTarget::Native,
        emit_native_host: false,
    };

    let mut pipeline = Pipeline::new();
    if let Some(parent) = file_path.parent() {
        pipeline = pipeline.with_source_dir(parent.to_path_buf());
    }
    pipeline = pipeline.with_source_path(file_path.display().to_string());

    pipeline
        .build(&combined, &options)
        // Render through the compiler's own diagnostic formatter against the
        // combined source, so the report shows the same message `miri build`
        // would rather than a debug dump of the error value.
        .map_err(|error| error.report_with_path(&combined, pipeline.source_path()))?;

    Ok(Artifact {
        _directory: directory,
        executable,
    })
}

/// Run one test by name and classify how its process ended.
pub fn execute_test(executable: &Path, test_name: &str) -> Execution {
    let output = match Command::new(executable).arg(test_name).output() {
        Ok(output) => output,
        Err(error) => return Execution::Fault(format!("could not run the test binary: {}", error)),
    };

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = output.status.signal() {
            return Execution::Killed(signal);
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    match output.status.code() {
        Some(0) => Execution::Succeeded,
        Some(harness::EXIT_NO_TEST_NAME) => {
            Execution::Fault("the test binary was given no test name".to_string())
        }
        Some(harness::EXIT_UNKNOWN_TEST) => Execution::Fault(format!(
            "the test binary does not know a test called '{}'",
            test_name
        )),
        Some(_) => Execution::Errored(stderr),
        None => Execution::Errored(stderr),
    }
}

/// Turn one execution into a reportable result, applying `@xfail` inversion.
///
/// For an `@xfail` test the meanings swap: failing is what it documents, and
/// passing means the pinned bug is fixed and the marker must be removed. A
/// runner fault is never inverted — no test ran, so there is nothing to invert.
pub fn to_test_result(display: &str, test: &TestMarker, execution: Execution) -> TestResult {
    let (outcome, detail) = match (execution, test.is_xfail()) {
        (Execution::Succeeded, false) => (Outcome::Passed, None),
        (Execution::Succeeded, true) => (Outcome::UnexpectedPass, unexpected_pass_detail(test)),
        (Execution::Errored(stderr), false) => (Outcome::Failed, non_empty(stderr)),
        (Execution::Errored(_), true) => (Outcome::ExpectedFailure, test.xfail_reason.clone()),
        (Execution::Killed(signal), false) => (Outcome::Crashed, Some(signal_detail(signal))),
        (Execution::Killed(_), true) => (Outcome::ExpectedFailure, test.xfail_reason.clone()),
        (Execution::Fault(reason), _) => (Outcome::RunnerFault, Some(reason)),
    };

    TestResult {
        path: display.to_string(),
        name: test.name.clone(),
        outcome,
        detail,
    }
}

fn unexpected_pass_detail(test: &TestMarker) -> Option<String> {
    let reason = test.xfail_reason.clone().unwrap_or_default();
    Some(format!(
        "marked @xfail(\"{}\") but passed; remove the marker if the bug is fixed",
        reason
    ))
}

fn signal_detail(signal: i32) -> String {
    format!("the test process was killed by signal {}", signal)
}

fn non_empty(stderr: String) -> Option<String> {
    if stderr.trim().is_empty() {
        None
    } else {
        Some(stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(name: &str) -> TestMarker {
        TestMarker {
            name: name.to_string(),
            ignore_reason: None,
            xfail_reason: None,
        }
    }

    fn expected_to_fail(name: &str) -> TestMarker {
        TestMarker {
            name: name.to_string(),
            ignore_reason: None,
            xfail_reason: Some("known bug".to_string()),
        }
    }

    #[test]
    fn a_clean_exit_passes() {
        let result = to_test_result("a.mi", &plain("t"), Execution::Succeeded);
        assert_eq!(result.outcome, Outcome::Passed);
    }

    #[test]
    fn a_non_zero_exit_fails_and_keeps_stderr() {
        let result = to_test_result(
            "a.mi",
            &plain("t"),
            Execution::Errored("assertion failed at a.mi:3".to_string()),
        );
        assert_eq!(result.outcome, Outcome::Failed);
        assert_eq!(result.detail.as_deref(), Some("assertion failed at a.mi:3"));
    }

    #[test]
    fn an_xfail_that_fails_is_an_expected_failure() {
        let result = to_test_result(
            "a.mi",
            &expected_to_fail("t"),
            Execution::Errored("boom".to_string()),
        );
        assert_eq!(result.outcome, Outcome::ExpectedFailure);
        assert!(result.outcome.is_success());
    }

    #[test]
    fn an_xfail_that_passes_is_an_unexpected_pass() {
        let result = to_test_result("a.mi", &expected_to_fail("t"), Execution::Succeeded);
        assert_eq!(result.outcome, Outcome::UnexpectedPass);
        assert!(!result.outcome.is_success());
    }

    #[test]
    fn a_signal_death_is_reported_as_a_crash() {
        let result = to_test_result("a.mi", &plain("t"), Execution::Killed(11));
        assert_eq!(result.outcome, Outcome::Crashed);
        assert!(result.detail.is_some_and(|d| d.contains("signal 11")));
    }

    #[test]
    fn an_xfail_that_crashes_is_still_an_expected_failure() {
        let result = to_test_result("a.mi", &expected_to_fail("t"), Execution::Killed(11));
        assert_eq!(result.outcome, Outcome::ExpectedFailure);
        assert!(result.outcome.is_success());
    }

    #[test]
    fn a_runner_fault_is_never_inverted_by_xfail() {
        let result = to_test_result(
            "a.mi",
            &expected_to_fail("t"),
            Execution::Fault("no test name".to_string()),
        );
        assert_eq!(result.outcome, Outcome::RunnerFault);
        assert!(!result.outcome.is_success());
    }
}
