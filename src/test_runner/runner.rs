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
/// Also handles reading the sidecar assertion report file if present.
pub fn execute_test(
    executable: &Path,
    test_name: &str,
    sidecar_path: Option<&Path>,
) -> (Execution, Option<crate::test_runner::AssertionFailure>) {
    let mut cmd = Command::new(executable);
    cmd.arg(test_name);

    // Set the sidecar path env var if provided
    if let Some(path) = sidecar_path {
        if let Some(path_str) = path.to_str() {
            cmd.env("MIRI_ASSERT_REPORT_PATH", path_str);
        }
    }

    let output = match cmd.output() {
        Ok(output) => output,
        Err(error) => {
            return (
                Execution::Fault(format!("could not run the test binary: {}", error)),
                None,
            )
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = output.status.signal() {
            return (Execution::Killed(signal), None);
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let execution = match output.status.code() {
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
    };

    // Try to read the sidecar file if it exists
    let failure = sidecar_path.and_then(read_assert_report);
    (execution, failure)
}

/// Read and parse a structured assertion failure report from a sidecar file.
/// Returns None if the file doesn't exist, is malformed, or rejected for safety reasons.
fn read_assert_report(path: &Path) -> Option<crate::test_runner::AssertionFailure> {
    use std::fs;

    // Safety: validate the path is a regular file before reading
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return None,
    };

    if !metadata.is_file() {
        return None;
    }

    // Cap file size to prevent DoS (64 KiB is ample for a structured report)
    const MAX_REPORT_SIZE: u64 = 65_536;
    if metadata.len() > MAX_REPORT_SIZE {
        return None;
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    parse_assert_report(&content)
}

/// Parse field header: key:len:
/// Returns (key, len, position_after_header) or None if malformed.
fn parse_field_header<'a>(
    bytes: &'a [u8],
    mut pos: usize,
    valid_keys: &[&str],
    seen_keys: &mut std::collections::HashSet<String>,
) -> Option<(&'a str, usize, usize)> {
    // Find first colon (key separator)
    let mut key_end = pos;
    while key_end < bytes.len() && bytes[key_end] != b':' {
        key_end += 1;
    }
    if key_end >= bytes.len() {
        return None;
    }

    let key = std::str::from_utf8(&bytes[pos..key_end]).ok()?;
    if !valid_keys.contains(&key) {
        return None; // Unknown key
    }
    if !seen_keys.insert(key.to_string()) {
        return None; // Duplicate key
    }

    pos = key_end + 1;

    // Find second colon (length separator)
    let mut len_end = pos;
    while len_end < bytes.len() && bytes[len_end] != b':' {
        len_end += 1;
    }
    if len_end >= bytes.len() {
        return None;
    }

    let len_str = std::str::from_utf8(&bytes[pos..len_end]).ok()?;
    let len: usize = len_str.parse().ok()?;
    let pos_after = len_end.checked_add(1)?;

    Some((key, len, pos_after))
}

/// Parse a structured assertion failure report from text.
/// Format: `<key>:<byte-len>:<raw bytes>\n` for each field.
///
/// The length prefix allows arbitrary byte sequences (including newlines)
/// in values without escaping. Unknown keys are rejected, and duplicate
/// keys are not allowed.
fn parse_assert_report(content: &str) -> Option<crate::test_runner::AssertionFailure> {
    use std::collections::HashSet;

    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut seen_keys: HashSet<String> = HashSet::new();

    let valid_keys = [
        "version",
        "code",
        "kind",
        "path",
        "line",
        "column",
        "expression",
        "expected",
        "actual",
        "message",
    ];

    let bytes = content.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        let (key, len, mut pos_after_header) =
            parse_field_header(bytes, pos, &valid_keys, &mut seen_keys)?;

        // Check bounds and extract exactly `len` bytes
        if pos_after_header
            .checked_add(len)
            .is_none_or(|sum| sum > bytes.len())
        {
            return None; // Not enough bytes or overflow
        }

        let value_bytes = &bytes[pos_after_header..pos_after_header + len];
        let value = String::from_utf8(value_bytes.to_vec()).ok()?;

        fields.insert(key.to_string(), value);

        pos_after_header = pos_after_header.checked_add(len)?;

        // Require newline delimiter
        if pos_after_header >= bytes.len() || bytes[pos_after_header] != b'\n' {
            return None; // Missing or incorrect delimiter
        }

        pos = pos_after_header.checked_add(1)?;
    }

    build_assertion_failure_from_fields(&fields)
}

/// Construct AssertionFailure from parsed fields after version validation.
fn build_assertion_failure_from_fields(
    fields: &std::collections::HashMap<String, String>,
) -> Option<crate::test_runner::AssertionFailure> {
    // Version check
    if fields.get("version").map(|v| v.as_str()) != Some("1") {
        return None;
    }

    // Extract required fields
    let code = fields.get("code")?.clone();
    let kind = fields.get("kind")?.clone();

    // Parse line and column, treating zero as absent (lines and columns are 1-indexed).
    let line: Option<usize> = fields
        .get("line")
        .and_then(|l| l.parse::<usize>().ok())
        .filter(|&n| n > 0);
    let column: Option<usize> = fields
        .get("column")
        .and_then(|c| c.parse::<usize>().ok())
        .filter(|&n| n > 0);

    Some(crate::test_runner::AssertionFailure {
        code,
        kind,
        line,
        column,
        expression: fields.get("expression").cloned().filter(|s| !s.is_empty()),
        expected: fields.get("expected").cloned().filter(|s| !s.is_empty()),
        actual: fields.get("actual").cloned().filter(|s| !s.is_empty()),
        message: fields.get("message").cloned().filter(|s| !s.is_empty()),
    })
}

/// Turn one execution into a reportable result, applying `@xfail` inversion.
///
/// For an `@xfail` test the meanings swap: failing is what it documents, and
/// passing means the pinned bug is fixed and the marker must be removed. A
/// runner fault is never inverted — no test ran, so there is nothing to invert.
pub fn to_test_result(
    display: &str,
    test: &TestMarker,
    execution: Execution,
    failure: Option<crate::test_runner::AssertionFailure>,
) -> TestResult {
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
        failure,
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
        let result = to_test_result("a.mi", &plain("t"), Execution::Succeeded, None);
        assert_eq!(result.outcome, Outcome::Passed);
    }

    #[test]
    fn a_non_zero_exit_fails_and_keeps_stderr() {
        let result = to_test_result(
            "a.mi",
            &plain("t"),
            Execution::Errored("assertion failed at a.mi:3".to_string()),
            None,
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
            None,
        );
        assert_eq!(result.outcome, Outcome::ExpectedFailure);
        assert!(result.outcome.is_success());
    }

    #[test]
    fn an_xfail_that_passes_is_an_unexpected_pass() {
        let result = to_test_result("a.mi", &expected_to_fail("t"), Execution::Succeeded, None);
        assert_eq!(result.outcome, Outcome::UnexpectedPass);
        assert!(!result.outcome.is_success());
    }

    #[test]
    fn a_signal_death_is_reported_as_a_crash() {
        let result = to_test_result("a.mi", &plain("t"), Execution::Killed(11), None);
        assert_eq!(result.outcome, Outcome::Crashed);
        assert!(result.detail.is_some_and(|d| d.contains("signal 11")));
    }

    #[test]
    fn an_xfail_that_crashes_is_still_an_expected_failure() {
        let result = to_test_result("a.mi", &expected_to_fail("t"), Execution::Killed(11), None);
        assert_eq!(result.outcome, Outcome::ExpectedFailure);
        assert!(result.outcome.is_success());
    }

    #[test]
    fn a_runner_fault_is_never_inverted_by_xfail() {
        let result = to_test_result(
            "a.mi",
            &expected_to_fail("t"),
            Execution::Fault("no test name".to_string()),
            None,
        );
        assert_eq!(result.outcome, Outcome::RunnerFault);
        assert!(!result.outcome.is_success());
    }

    #[test]
    fn parse_assert_report_rejects_usize_max_length() {
        // Overflow attack: usize::MAX as length should not cause panic
        // The length is a string field followed by the length of that string
        let length_value = usize::MAX.to_string(); // e.g., "18446744073709551615"
        let malicious = format!(
            "version:1:1\ncode:{}:{}\n",
            length_value.len(),
            length_value
        );
        let result = parse_assert_report(&malicious);
        assert_eq!(
            result, None,
            "should reject field with usize::MAX length without panicking"
        );
    }

    #[test]
    fn parse_assert_report_rejects_truncated_record() {
        let truncated = "version:1:1\ncode:";
        let result = parse_assert_report(truncated);
        assert_eq!(result, None, "should reject truncated record");
    }

    #[test]
    fn parse_assert_report_rejects_bad_version() {
        let bad_version = "version:1:2\n";
        let result = parse_assert_report(bad_version);
        assert_eq!(result, None, "should reject bad version");
    }

    #[test]
    fn parse_assert_report_rejects_trailing_garbage() {
        let garbage = "version:1:1\ncode:7:MER_RT_005\nkind:6:assert\nextra garbage here";
        let result = parse_assert_report(garbage);
        assert_eq!(result, None, "should reject unknown key");
    }

    #[test]
    fn parse_assert_report_handles_embedded_newlines() {
        // Message containing a newline should not break parsing
        let with_newline =
            "version:1:1\ncode:10:MER_RT_005\nkind:6:assert\nmessage:11:line1\nline2\n";
        let result = parse_assert_report(with_newline);
        assert!(
            result.is_some(),
            "should accept message with embedded newline"
        );
        let failure = result.unwrap();
        assert_eq!(failure.message.as_deref(), Some("line1\nline2"));
    }
}
