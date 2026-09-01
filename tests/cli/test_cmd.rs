// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use predicates::prelude::PredicateBooleanExt;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

/// Writes one `.mi` file into a fresh directory and returns the directory.
///
/// The directory is returned rather than the path because it owns the
/// temporary tree: dropping it removes the file.
fn test_dir_with(name: &str, source: &str) -> TempDir {
    let dir = TempDir::new().expect("a temporary directory");
    write_file(dir.path(), name, source);
    dir
}

fn write_file(dir: &Path, name: &str, source: &str) {
    let mut file = std::fs::File::create(dir.join(name)).expect("a test source file");
    write!(file, "{}", source).expect("the source is written");
}

/// Assertions come from `system.testing`, so a test file that asserts must
/// import it. Without the import the file fails to *compile*, which would make
/// a test asserting on failure pass for entirely the wrong reason.
const TESTING_IMPORT: &str = "use system.testing\n\n";

#[test]
fn test_test_command_help() {
    let mut cmd = miri_cmd();
    cmd.arg("test").arg("--help").assert().success();
}

#[test]
fn test_directory_without_any_tests_is_green() {
    let dir = test_dir_with("plain.mi", "fn helper()\n    var x = 1\n");

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("running 0 tests"))
        .stdout(predicates::str::contains("test result: ok"))
        // A run with nothing to report must not print an empty failures block.
        .stdout(predicates::str::contains("failures:").not());
}

#[test]
fn test_passing_test_is_reported_ok() {
    let dir = test_dir_with(
        "math.mi",
        &format!(
            "{}@test\nfn test_adds()\n    assert(1 + 1 == 2)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test math.mi::test_adds ... ok"))
        .stdout(predicates::str::contains(
            "test result: ok. 1 passed; 0 failed; 0 ignored",
        ));
}

/// The failure must carry the assertion's own `path:line` message, which is
/// what proves the test really ran and really failed its assertion rather than
/// failing to compile.
#[test]
fn test_failing_assertion_reports_its_source_location() {
    let dir = test_dir_with(
        "math.mi",
        &format!(
            "{}@test\nfn test_fails()\n    assert(1 == 2)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains(
            "test math.mi::test_fails ... FAILED",
        ))
        .stdout(predicates::str::contains("---- math.mi::test_fails ----"))
        .stdout(predicates::str::contains("assertion failed at"))
        .stdout(predicates::str::contains("math.mi:5"))
        .stdout(predicates::str::contains(
            "test result: FAILED. 0 passed; 1 failed; 0 ignored",
        ));
}

/// A failing assertion terminates its own process, so the isolation boundary is
/// what lets a later test still run.
#[test]
fn test_a_failure_does_not_abort_the_remaining_tests() {
    let dir = test_dir_with(
        "math.mi",
        &format!(
            "{}@test\nfn test_first_fails()\n    assert(1 == 2)\n\n@test\nfn test_second_runs()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains(
            "test math.mi::test_first_fails ... FAILED",
        ))
        .stdout(predicates::str::contains(
            "test math.mi::test_second_runs ... ok",
        ))
        .stdout(predicates::str::contains(
            "test result: FAILED. 1 passed; 1 failed; 0 ignored",
        ));
}

/// The body asserts something false, so reporting it as ignored rather than
/// failed is only possible if it was never executed.
#[test]
fn test_ignored_test_is_skipped_and_shows_its_reason() {
    let dir = test_dir_with(
        "skipped.mi",
        &format!(
            "{}@test\n@ignore(\"flaky on CI\")\nfn test_skipped()\n    assert(1 == 2)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "test skipped.mi::test_skipped ... ignored, flaky on CI",
        ))
        .stdout(predicates::str::contains(
            "test result: ok. 0 passed; 0 failed; 1 ignored",
        ));
}

#[test]
fn test_xfail_test_that_fails_keeps_the_run_green() {
    let dir = test_dir_with(
        "bug.mi",
        &format!(
            "{}@test\n@xfail(\"known bug\")\nfn test_known_bug()\n    assert(1 == 2)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "test bug.mi::test_known_bug ... ok (expected failure)",
        ))
        .stdout(predicates::str::contains("test result: ok. 1 passed"));
}

#[test]
fn test_xfail_test_that_passes_fails_the_run() {
    let dir = test_dir_with(
        "fixed.mi",
        &format!(
            "{}@test\n@xfail(\"should still be broken\")\nfn test_now_works()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("FAILED (unexpected pass)"))
        .stdout(predicates::str::contains("remove the marker"))
        .stdout(predicates::str::contains("test result: FAILED."));
}

/// The appended dispatcher declares `main`, so a file bringing its own would
/// produce a duplicate-symbol dump out of codegen. The runner refuses first.
#[test]
fn test_file_declaring_main_is_refused_not_silently_skipped() {
    let dir = test_dir_with(
        "has_main.mi",
        &format!(
            "{}@test\nfn test_a()\n    assert(1 == 1)\n\nfn main() int\n    return 0\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("not run:"))
        .stdout(predicates::str::contains("---- has_main.mi ----"))
        .stdout(predicates::str::contains("declares its own `main`"))
        .stdout(predicates::str::contains("1 file(s) not run"))
        // The raw codegen error must never reach the user.
        .stdout(predicates::str::contains("Duplicate definition").not());
}

/// Script-mode wrapping is skipped once a `main` exists, so these statements
/// would be dropped without a word. Refusing the file is what makes that
/// visible.
#[test]
fn test_file_with_top_level_statements_is_refused() {
    let dir = test_dir_with(
        "toplevel.mi",
        &format!(
            "{}println(\"top level\")\n\n@test\nfn test_a()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("---- toplevel.mi ----"))
        .stdout(predicates::str::contains(
            "has executable statements outside a function",
        ));
}

/// A test file whose syntax broke must not disappear into "0 tests, ok".
#[test]
fn test_unparseable_file_declaring_tests_is_refused() {
    let dir = test_dir_with(
        "broken.mi",
        &format!(
            "{}@test\nfn test_broken(\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("---- broken.mi ----"))
        .stdout(predicates::str::contains("does not parse"));
}

/// A file that neither parses nor mentions `@test` is simply not a test file.
/// The repository ships deliberately invalid `.mi` fixtures, and they must not
/// turn every run red.
#[test]
fn test_unparseable_file_without_tests_is_ignored_quietly() {
    let dir = test_dir_with("notatest.mi", "fn broken(\n");

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("running 0 tests"))
        .stdout(predicates::str::contains("test result: ok"));
}

#[test]
fn test_compile_error_is_rendered_as_a_diagnostic() {
    // No `use system.testing`, so `assert` is undefined.
    let dir = test_dir_with("noimport.mi", "@test\nfn test_a()\n    assert(1 == 1)\n");

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("error[MER_TYP_034]"))
        .stdout(predicates::str::contains("Undefined variable: assert"))
        // The span must point at the user's own line, not into the appended
        // dispatcher, and never at a debug dump of the error value.
        .stdout(predicates::str::contains("noimport.mi:3:5"))
        .stdout(predicates::str::contains("TypeError {").not());
}

#[test]
fn test_filter_selects_by_test_name() {
    let dir = test_dir_with(
        "math.mi",
        &format!(
            "{}@test\nfn test_adds()\n    assert(1 == 1)\n\n@test\nfn test_divides()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--filter")
        .arg("divides")
        .assert()
        .success()
        .stdout(predicates::str::contains("test_divides"))
        .stdout(predicates::str::contains("test_adds").not())
        .stdout(predicates::str::contains("test result: ok. 1 passed"));
}

#[test]
fn test_json_format_reports_outcomes_and_counts() {
    let dir = test_dir_with(
        "math.mi",
        &format!(
            "{}@test\nfn test_ok()\n    assert(1 == 1)\n\n@test\n@xfail(\"known\")\nfn test_bug()\n    assert(1 == 2)\n",
            TESTING_IMPORT
        ),
    );

    let output = miri_cmd()
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("the test command runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("the output parses as JSON");

    // Verify envelope structure
    assert_eq!(parsed["command"], "test");
    assert!(parsed["ok"].is_boolean());
    assert!(parsed["schemaVersion"].is_number());
    assert!(parsed["exitCode"].is_number());
    assert!(parsed["durationMs"].is_number());

    // Test summary is nested under "tests"
    let tests = &parsed["tests"];
    assert!(tests.is_object());

    assert_eq!(tests["total"], 2);
    assert_eq!(tests["passed"], 2);
    assert_eq!(tests["failed"], 0);
    assert_eq!(tests["ignored"], 0);
    assert_eq!(tests["rejectedFiles"].as_array().map(Vec::len), Some(0));

    let results = tests["results"].as_array().expect("results is an array");
    assert_eq!(results.len(), 2);
    for entry in results {
        assert!(entry["path"].is_string());
        assert!(entry["name"].is_string());
        assert!(entry["outcome"].is_string());
        // `detail` is present when there is something to say (e.g., ignore reason, error output),
        // or absent when the test simply passed or failed.
    }
    assert_eq!(results[0]["outcome"], "passed");
    assert_eq!(results[1]["outcome"], "expected_failure");
}

/// Two files in one directory each get their own compile and their own report
/// lines, so discovery is not limited to a single file.
#[test]
fn test_tests_are_discovered_across_multiple_files() {
    let dir = TempDir::new().expect("a temporary directory");
    write_file(
        dir.path(),
        "first.mi",
        &format!(
            "{}@test\nfn test_one()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );
    write_file(
        dir.path(),
        "second.mi",
        &format!(
            "{}@test\nfn test_two()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test_one"))
        .stdout(predicates::str::contains("test_two"))
        .stdout(predicates::str::contains("test result: ok. 2 passed"));
}

#[test]
fn test_filter_matching_nothing_runs_no_tests() {
    let dir = test_dir_with(
        "math.mi",
        &format!(
            "{}@test\nfn test_adds()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--filter")
        .arg("no_such_test")
        .assert()
        .success()
        .stdout(predicates::str::contains("running 0 tests"))
        .stdout(predicates::str::contains("test_adds").not())
        .stdout(predicates::str::contains("test result: ok"));
}

/// `@ignore` wins over `@xfail`: an ignored test is never spawned, so there is
/// no outcome for the expected-failure rule to invert. The body asserts
/// something false, which is what proves it did not run.
#[test]
fn test_ignore_takes_precedence_over_xfail() {
    let dir = test_dir_with(
        "both.mi",
        &format!(
            "{}@test\n@ignore(\"flaky on CI\")\n@xfail(\"also broken\")\nfn test_both()\n    assert(1 == 2)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "test both.mi::test_both ... ignored, flaky on CI",
        ))
        .stdout(predicates::str::contains("expected failure").not())
        .stdout(predicates::str::contains(
            "test result: ok. 0 passed; 0 failed; 1 ignored",
        ));
}

#[test]
fn test_tests_are_discovered_in_nested_directories() {
    let dir = TempDir::new().expect("a temporary directory");
    let nested = dir.path().join("suite");
    std::fs::create_dir(&nested).expect("a nested directory");

    write_file(
        dir.path(),
        "top.mi",
        &format!(
            "{}@test\nfn test_top()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );
    write_file(
        &nested,
        "inner.mi",
        &format!(
            "{}@test\nfn test_inner()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("top.mi::test_top"))
        // The nested path is shown relative to the searched directory.
        .stdout(predicates::str::contains("suite/inner.mi::test_inner"))
        .stdout(predicates::str::contains("test result: ok. 2 passed"));
}

/// A file that will not compile fails every test it declares, not just the
/// first one — otherwise a broken file would report a partial pass.
#[test]
fn test_compile_error_fails_every_test_in_the_file() {
    // No `use system.testing`, so `assert` is undefined for both tests.
    let dir = test_dir_with(
        "broken.mi",
        "@test\nfn test_a()\n    assert(1 == 1)\n\n@test\nfn test_b()\n    assert(1 == 1)\n",
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("error[MER_TYP_034]"))
        .stdout(predicates::str::contains(
            "test result: FAILED. 0 passed; 2 failed; 0 ignored",
        ));
}

/// Nothing needs compiling when every test in a file is ignored. The file
/// would not compile if it were built, so a clean ignored run proves the
/// compile was skipped.
#[test]
fn test_file_with_only_ignored_tests_is_never_compiled() {
    let dir = test_dir_with(
        "allignored.mi",
        "@test\n@ignore(\"not ready\")\nfn test_a()\n    assert(1 == 1)\n",
    );

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("ignored, not ready"))
        .stdout(predicates::str::contains("error[MER_TYP_034]").not())
        .stdout(predicates::str::contains(
            "test result: ok. 0 passed; 0 failed; 1 ignored",
        ));
}

// Tests for structured assertion failure reporting in JSON format

#[test]
fn test_json_assert_eq_failure_has_structured_fields() {
    let dir = test_dir_with(
        "assert_eq_test.mi",
        &format!(
            "{}@test\nfn test_eq_fails()\n    assert_eq(42, 41, \"off by one\")\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    // Verify structure exists and has fields
    let results = json["tests"]["results"]
        .as_array()
        .expect("results should be an array");
    let result = results.first().expect("should have at least one result");

    assert_eq!(result["outcome"].as_str(), Some("failed"));
    assert_eq!(result["code"].as_str(), Some("MER_RT_005"));
    assert!(result["line"].is_number(), "line should be a number");
    assert!(result["column"].is_number(), "column should be a number");
    assert_eq!(result["expected"].as_str(), Some("41"));
    assert_eq!(result["actual"].as_str(), Some("42"));
    assert_eq!(result["message"].as_str(), Some("off by one"));

    // Verify exit code
    assert_eq!(
        output.status.code(),
        Some(1),
        "exit code should be 1 for test failure"
    );
    assert_eq!(
        json["exitCode"].as_i64(),
        Some(1),
        "JSON exitCode should also be 1"
    );
}

#[test]
fn test_json_assert_ne_failure_has_structured_fields() {
    let dir = test_dir_with(
        "assert_ne_test.mi",
        &format!(
            "{}@test\nfn test_ne_fails()\n    assert_ne(5, 5, \"values must differ\")\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let results = json["tests"]["results"]
        .as_array()
        .expect("results should be an array");
    let result = results.first().expect("should have at least one result");

    assert_eq!(result["outcome"].as_str(), Some("failed"));
    assert_eq!(result["code"].as_str(), Some("MER_RT_005"));
    assert!(result["line"].is_number(), "line should be a number");
    assert!(result["column"].is_number(), "column should be a number");
    // assert_ne does not report expected value, only actual
    assert!(
        result["expected"].is_null(),
        "expected should not be present for assert_ne"
    );
    assert_eq!(result["actual"].as_str(), Some("5"));
    assert_eq!(result["message"].as_str(), Some("values must differ"));
}

#[test]
fn test_json_bare_assert_has_expression_text() {
    let dir = test_dir_with(
        "bare_assert_test.mi",
        &format!(
            "{}@test\nfn test_bare_assert()\n    assert(1 == 2, \"should be equal\")\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let result = json["tests"]["results"][0].clone();
    assert_eq!(result["outcome"].as_str(), Some("failed"));
    // The expression text should be exactly the condition from the assert
    assert_eq!(
        result["expression"].as_str(),
        Some("1 == 2"),
        "expression should be the exact condition"
    );
    assert_eq!(
        result["message"].as_str(),
        Some("should be equal"),
        "message should be preserved"
    );
}

#[test]
fn test_json_passing_test_has_no_structured_fields() {
    let dir = test_dir_with(
        "passing_test.mi",
        &format!(
            "{}@test\nfn test_passes()\n    assert(1 == 1)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let result = json["tests"]["results"][0].clone();
    assert_eq!(result["outcome"].as_str(), Some("passed"));
    // Passing tests should not have these fields
    assert!(
        result.get("code").is_none() || result["code"].is_null(),
        "passing test should not have code"
    );
    assert!(
        result.get("line").is_none() || result["line"].is_null(),
        "passing test should not have line"
    );
    assert!(
        result.get("expression").is_none() || result["expression"].is_null(),
        "passing test should not have expression"
    );

    // Process exit code should be 0
    assert_eq!(
        output.status.code(),
        Some(0),
        "exit code should be 0 for passing"
    );
    assert_eq!(
        json["exitCode"].as_i64(),
        Some(0),
        "JSON exitCode should be 0"
    );
}

#[test]
fn test_json_assert_panics_closure_did_not_panic() {
    let dir = test_dir_with(
        "assert_panics_test.mi",
        &format!(
            "{}@test\nfn test_panics_fails()\n    assert_panics(fn(): println(\"hi\"))\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let result = json["tests"]["results"][0].clone();
    assert_eq!(result["outcome"].as_str(), Some("failed"));
    assert_eq!(result["code"].as_str(), Some("MER_RT_005"));
    assert!(result["line"].is_number(), "line should be a number");
    assert!(result["column"].is_number(), "column should be a number");
    // assert_panics has a message when closure doesn't panic
    assert!(result["message"].is_string(), "message should be present");
}

#[test]
fn test_json_assert_panics_message_mismatch() {
    let dir = test_dir_with(
        "assert_panics_msg_test.mi",
        &format!(
            "{}@test\nfn test_panics_msg_fails()\n    assert_panics(fn(): panic(\"wrong\"), \"expected\")\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let result = json["tests"]["results"][0].clone();
    assert_eq!(result["outcome"].as_str(), Some("failed"));
    assert_eq!(result["code"].as_str(), Some("MER_RT_005"));
    assert!(result["line"].is_number(), "line should be a number");
    assert!(result["column"].is_number(), "column should be a number");
    // assert_panics with message mismatch has expected and actual
    assert_eq!(
        result["expected"].as_str(),
        Some("expected"),
        "expected message should be in JSON"
    );
    assert_eq!(
        result["actual"].as_str(),
        Some("wrong"),
        "actual panic message should be in JSON"
    );
}

#[test]
fn test_exit_code_2_when_file_rejected() {
    let dir = test_dir_with(
        "unparseable.mi",
        "this is not valid miri code }{][\n@test\nfn test()\n",
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    // Exit code should be 2 (rejected file takes priority)
    assert_eq!(
        output.status.code(),
        Some(2),
        "exit code should be 2 when file is rejected"
    );

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    assert_eq!(
        json["exitCode"].as_i64(),
        Some(2),
        "JSON exitCode should also be 2"
    );

    // Should have rejected files
    assert!(
        json["tests"]["rejectedFiles"].as_array().unwrap().len() > 0,
        "should have rejected files"
    );
}

#[test]
fn test_column_is_1_indexed_and_accurate() {
    let dir = test_dir_with(
        "column_test.mi",
        &format!(
            "{}@test\nfn test_column()\n    var x = 5\n    assert_eq(x, 6)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let result = json["tests"]["results"][0].clone();
    let column = result["column"]
        .as_u64()
        .expect("column should be a positive number");
    // Column should be 5: 4 spaces (1-4) + 'a' (5) in "assert_eq"
    assert_eq!(
        column, 5,
        "column should point to the start of assert_eq (1-indexed)"
    );
}

#[test]
fn test_column_scales_with_indent() {
    let dir = test_dir_with(
        "column_deep_test.mi",
        &format!(
            "{}@test\nfn test_column_deep()\n    if true:\n        if true:\n            assert_eq(7, 8)\n",
            TESTING_IMPORT
        ),
    );

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("test")
        .arg("--dir")
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("test command should run");

    let json_text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&json_text).expect("output should be valid JSON");

    let result = json["tests"]["results"][0].clone();
    let column = result["column"]
        .as_u64()
        .expect("column should be a positive number");
    // Column should be 13: 12 spaces (1-12) + 'a' (13) in "assert_eq" at deepest indent
    assert_eq!(
        column, 13,
        "column should point to start of assert_eq at 12-space indent (1-indexed)"
    );
}

/// A run that names one file runs that file and nothing else. A sibling whose
/// name contains the named one is not swept in, which is what a caller asked
/// for by naming a file rather than the directory holding it.
#[test]
fn test_a_named_file_runs_alone() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("a.mi"),
        "use system.testing\n\n@test\nfn test_alpha()\n    assert_eq(1, 1)\n",
    )
    .unwrap();
    // `xa.mi` contains `a.mi`, so selecting by substring would run it too.
    std::fs::write(
        directory.path().join("xa.mi"),
        "use system.testing\n\n@test\nfn test_gamma()\n    assert_eq(3, 3)\n",
    )
    .unwrap();

    let output = miri_cmd()
        .arg("test")
        .arg(directory.path().join("a.mi"))
        .arg("--format")
        .arg("json")
        .output()
        .expect("the test command runs");

    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("the output parses as JSON");

    assert_eq!(parsed["tests"]["total"], 1);
    assert_eq!(parsed["tests"]["passed"], 1);
    assert_eq!(parsed["tests"]["results"][0]["name"], "test_alpha");
    assert_eq!(output.status.code(), Some(0));
}

/// The directory form still walks, whether it is named positionally or with
/// the older `--dir` spelling.
#[test]
fn test_a_named_directory_walks_the_way_dir_does() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("a.mi"),
        "use system.testing\n\n@test\nfn test_alpha()\n    assert_eq(1, 1)\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("b.mi"),
        "use system.testing\n\n@test\nfn test_beta()\n    assert_eq(2, 2)\n",
    )
    .unwrap();

    let total = |args: &[&str]| -> i64 {
        let output = miri_cmd()
            .args(args)
            .arg("--format")
            .arg("json")
            .output()
            .expect("the test command runs");
        let parsed: serde_json::Value =
            serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
                .expect("the output parses as JSON");
        parsed["tests"]["total"].as_i64().expect("a count")
    };

    let path = directory.path().to_str().expect("a printable path");
    assert_eq!(total(&["test", path]), 2);
    assert_eq!(total(&["test", "--dir", path]), 2);
}

/// A named file that cannot host a dispatcher is still a rejection, and a
/// rejection still outranks everything: the tests in it never ran, so the run
/// is incomplete rather than merely red.
#[test]
fn test_a_named_file_that_is_rejected_exits_two() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.mi");
    std::fs::write(
        &path,
        "use system.testing\n\n@test\nfn test_broken(\n    assert(1 == 1)\n",
    )
    .unwrap();

    let output = miri_cmd()
        .arg("test")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("the test command runs");

    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("the output parses as JSON");

    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["exitCode"], 2);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(parsed["tests"]["rejectedFiles"][0]["reason"], "unparseable");
}

/// A path that names nothing discovers no tests, and a run of no tests is
/// green. Reporting a mistyped path as a passing suite is the one answer this
/// command must not give.
#[test]
fn test_a_path_that_does_not_exist_is_reported_not_green() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("absent.mi");

    let output = miri_cmd()
        .arg("test")
        .arg(&missing)
        .arg("--format")
        .arg("json")
        .output()
        .expect("the test command runs");

    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("the output parses as JSON");

    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["diagnostics"][0]["code"], "MER_BLD_008");
    assert_eq!(output.status.code(), Some(1));
}

/// Naming one file still resolves its imports from the directory holding it,
/// so a test that uses a sibling module runs the same way it does under a walk.
#[test]
fn test_a_named_file_resolves_its_sibling_imports() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("util.mi"),
        "public fn helper() int:\n    return 42\n",
    )
    .unwrap();
    let path = directory.path().join("main.mi");
    std::fs::write(
        &path,
        "use system.testing\nuse util\n\n@test\nfn test_helper()\n    assert_eq(helper(), 42)\n",
    )
    .unwrap();

    let output = miri_cmd()
        .arg("test")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("the test command runs");

    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("the output parses as JSON");

    assert_eq!(parsed["tests"]["passed"], 1, "{}", parsed);
    assert_eq!(output.status.code(), Some(0));
}
