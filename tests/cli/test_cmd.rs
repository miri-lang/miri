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
