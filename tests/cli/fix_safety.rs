// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for fix-safety taxonomy and repair refusal.

use crate::utils::miri_cmd;
use std::fs;
use std::path::Path;

/// A source file in a directory of its own, removed when the test ends.
struct Fixture {
    directory: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("miri-fix-safety-{}", name));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("could not create the fixture directory");
        let file = directory.join("main.mi");
        fs::write(&file, source).expect("could not write the fixture source");
        Self { directory, file }
    }

    fn path(&self) -> &Path {
        &self.file
    }

    fn contents(&self) -> String {
        fs::read_to_string(&self.file).expect("could not read the fixture source")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn test_function_local_let_reassignment_accepted_without_allow_risky() {
    let fixture = Fixture::new(
        "function_local",
        r#"fn main()
    let counter = 0
    counter = counter + 1
    println(counter)
"#,
    );

    // Function-local repair should apply without --allow-risky
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply");

    assert!(
        output.status.success(),
        "Function-local repair should be applied without --allow-risky. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the repair was applied
    let modified = fixture.contents();
    assert!(
        modified.contains("var counter"),
        "Function-local repair should change `let` to `var`. Got:\n{}",
        modified
    );
}

#[test]
fn test_module_scope_let_reassignment_refused_without_allow_risky() {
    let fixture = Fixture::new(
        "module_scope",
        r#"let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    let original = fixture.contents();

    // Module-scope repair should be refused without --allow-risky
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply");

    assert!(
        !output.status.success(),
        "Should refuse module-scope repair without --allow-risky"
    );

    // Verify the refusal message appears in stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("MER_BLD_002"),
        "Refusal message should mention the MER_BLD_002 code. stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("Refused"),
        "Refusal message should explain the refusal. stderr: {}",
        stderr
    );

    // Verify the file was NOT modified
    let unchanged = fixture.contents();
    assert_eq!(
        unchanged, original,
        "File should not be modified when repair is refused"
    );
}

#[test]
fn test_module_scope_let_reassignment_accepted_with_allow_risky() {
    let fixture = Fixture::new(
        "module_scope_allowed",
        r#"let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    // Apply with --allow-risky
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg("--allow-risky")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply --allow-risky");

    assert!(
        output.status.success(),
        "Should apply module-scope repair with --allow-risky. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the repair was applied
    let modified = fixture.contents();
    assert!(
        modified.contains("var counter"),
        "Module-scope repair should change `let` to `var` with --allow-risky. Got:\n{}",
        modified
    );

    // Verify the repaired file compiles
    let check_output = miri_cmd()
        .arg("check")
        .arg(fixture.path())
        .output()
        .expect("run miri check on repaired file");

    assert!(
        check_output.status.success(),
        "Repaired file should compile. stderr: {}",
        String::from_utf8_lossy(&check_output.stderr)
    );
}

#[test]
fn test_plan_never_refuses() {
    let fixture = Fixture::new(
        "plan_never_refuses",
        r#"let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    // Plan should succeed
    let output = miri_cmd()
        .arg("fix")
        .arg("--plan")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --plan");

    assert!(output.status.success(), "--plan should never refuse");

    // Verify the plan contains repair details
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty() && (stdout.contains("repair") || stdout.contains("repair")),
        "Plan output should contain repair information. Got: {}",
        stdout
    );

    // Plan with --allow-risky should also succeed
    let output2 = miri_cmd()
        .arg("fix")
        .arg("--plan")
        .arg("--allow-risky")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --plan --allow-risky");

    assert!(
        output2.status.success(),
        "--plan --allow-risky should never refuse"
    );
}

#[test]
fn test_plan_contains_repairs() {
    let fixture = Fixture::new(
        "plan_contains_repairs",
        r#"let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    // Plan should output details about the repair
    let output = miri_cmd()
        .arg("fix")
        .arg("--plan")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --plan");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("repair") || stdout.contains("Declare"),
        "Plan output should contain repair details. Got: {}",
        stdout
    );
}

#[test]
fn test_private_module_scope_let_reassignment_accepted_without_allow_risky() {
    let fixture = Fixture::new(
        "private_module_scope",
        r#"private let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    // Private module-scope binding should be accepted without --allow-risky
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply");

    assert!(
        output.status.success(),
        "Private module-scope repair should be applied without --allow-risky. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the repair was applied
    let modified = fixture.contents();
    assert!(
        modified.contains("var counter"),
        "Private module-scope repair should change `let` to `var`. Got:\n{}",
        modified
    );
}

#[test]
fn test_public_module_scope_let_reassignment_refused_without_allow_risky() {
    let fixture = Fixture::new(
        "public_module_scope",
        r#"public let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    let original = fixture.contents();

    // Public module-scope repair should be refused without --allow-risky
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply");

    assert!(
        !output.status.success(),
        "Should refuse public module-scope repair without --allow-risky"
    );

    // Verify the file was NOT modified
    let unchanged = fixture.contents();
    assert_eq!(
        unchanged, original,
        "File should not be modified when public repair is refused"
    );
}

#[test]
fn test_mixed_safe_and_risky_repairs_refuses_whole_command() {
    let fixture = Fixture::new(
        "mixed_safe_risky",
        r#"public let api_counter = 0

fn test_function()
    let local_counter = 0
    local_counter = local_counter + 1

fn bump()
    api_counter = api_counter + 1

fn main()
    test_function()
    bump()
"#,
    );

    let original = fixture.contents();

    // Command should refuse even though one repair is safe
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply");

    assert!(
        !output.status.success(),
        "Should refuse mixed safe/risky repairs without --allow-risky"
    );

    // Verify the file was NOT modified at all - even the safe repair should not be applied
    let unchanged = fixture.contents();
    assert_eq!(
        unchanged, original,
        "File should not be modified when any repair is refused"
    );

    // Verify MER_BLD_002 appears in stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("MER_BLD_002"),
        "Refusal should mention MER_BLD_002. stderr: {}",
        stderr
    );
}

#[test]
fn test_apply_with_no_repairs_succeeds() {
    let fixture = Fixture::new(
        "no_repairs",
        r#"fn main()
    println("Hello, World!")
"#,
    );

    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply");

    assert!(
        output.status.success(),
        "--apply with no repairs should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_json_refusal_includes_envelope() {
    let fixture = Fixture::new(
        "json_refusal",
        r#"public let counter = 0

fn bump()
    counter = counter + 1

fn main()
    bump()
"#,
    );

    // Request JSON output with a refusal case
    let output = miri_cmd()
        .arg("fix")
        .arg("--apply")
        .arg("--yes")
        .arg("--format")
        .arg("json")
        .arg(fixture.path())
        .output()
        .expect("run miri fix --apply --format json");

    // Should not succeed (refusal)
    assert!(!output.status.success());

    // Should emit JSON to stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "JSON refusal should emit to stdout, not just stderr"
    );

    // Should be valid JSON with DiagnosticsEnvelope structure
    let json: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(json.is_ok(), "JSON output must parse. Got: {}", stdout);

    let envelope = json.unwrap();
    assert_eq!(
        envelope.get("command").and_then(|v| v.as_str()),
        Some("fix"),
        "Envelope command must be 'fix'"
    );
    assert_eq!(
        envelope.get("ok").and_then(|v| v.as_bool()),
        Some(false),
        "Envelope ok must be false for refusal"
    );

    // Should contain the refusal code in diagnostics
    let diagnostics = envelope.get("diagnostics").and_then(|v| v.as_array());
    assert!(
        diagnostics.is_some(),
        "Envelope must have diagnostics array"
    );
    let has_refusal_code = diagnostics
        .unwrap()
        .iter()
        .any(|d| d.get("code").and_then(|c| c.as_str()) == Some("MER_BLD_002"));
    assert!(
        has_refusal_code,
        "JSON envelope should include MER_BLD_002 in diagnostics"
    );
}
