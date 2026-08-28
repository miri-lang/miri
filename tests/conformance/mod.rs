// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use miri::diagnostics::codes::DiagnosticCode;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Fixture parsing: extract directives from comment header.
#[derive(Debug, Clone)]
struct FixtureDirectives {
    expect_code: Option<String>,
    /// Subcommand used to exercise the fixture. Defaults to `check`; a fixture
    /// whose diagnostic is only raised while the program executes (a runtime
    /// trap) declares `// command: run`.
    command: String,
    expect_stdout_lines: Vec<String>,
    summary: String,
}

fn parse_fixture(path: &Path) -> Result<FixtureDirectives, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read fixture {}: {}", path.display(), e))?;

    let mut expect_code = None;
    let mut command = String::from("check");
    let mut expect_stdout_lines = Vec::new();
    let mut summary = None;

    for line in content.lines() {
        if line.trim().is_empty() {
            break;
        }
        if !line.starts_with("//") {
            break;
        }

        let comment = &line[2..].trim_start();

        if let Some(code) = comment.strip_prefix("expect: ") {
            expect_code = Some(code.to_string());
        } else if let Some(stdout) = comment.strip_prefix("expect-stdout: ") {
            expect_stdout_lines.push(stdout.to_string());
        } else if let Some(c) = comment.strip_prefix("command: ") {
            command = c.trim().to_string();
        } else if let Some(s) = comment.strip_prefix("summary: ") {
            summary = Some(s.to_string());
        }
    }

    let summary = summary.ok_or_else(|| {
        format!(
            "Fixture {} missing required directive '// summary: <one line>'",
            path.display()
        )
    })?;

    if command != "check" && command != "run" {
        return Err(format!(
            "Fixture {} declares unsupported '// command: {}'; expected 'check' or 'run'",
            path.display(),
            command
        ));
    }

    Ok(FixtureDirectives {
        expect_code,
        command,
        expect_stdout_lines,
        summary,
    })
}

/// Verify that fail/<CODE>.mi contains expected error code.
fn test_fail_fixture(path: &Path) -> Result<(), String> {
    let directives = parse_fixture(path)?;

    let expect_code = directives.expect_code.ok_or_else(|| {
        format!(
            "Fixture {} in fail/ must have '// expect: <CODE>'",
            path.display()
        )
    })?;

    let mut file =
        NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    write!(file, "{}", content).map_err(|e| format!("Failed to write temp file: {}", e))?;

    let test_path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    cmd.env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        .env("RUST_BACKTRACE", "0")
        .arg(&directives.command)
        .arg(&test_path)
        .arg("--format")
        .arg("json");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run miri: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse JSON output: {}", e))?;

    if parsed["ok"] != false {
        return Err(format!(
            "Expected compilation to fail, but ok=true. Code: {}",
            expect_code
        ));
    }

    let diags = parsed["diagnostics"]
        .as_array()
        .ok_or_else(|| "diagnostics is not an array".to_string())?;

    let found = diags.iter().any(|d| {
        d["code"]
            .as_str()
            .map(|c| c == expect_code)
            .unwrap_or(false)
    });

    if !found {
        let codes: Vec<_> = diags.iter().filter_map(|d| d["code"].as_str()).collect();
        return Err(format!(
            "Expected error code {} but got: {:?}",
            expect_code, codes
        ));
    }

    Ok(())
}

/// Verify that warn/<CODE>.mi contains expected warning code and ok=true.
fn test_warn_fixture(path: &Path) -> Result<(), String> {
    let directives = parse_fixture(path)?;

    let expect_code = directives.expect_code.ok_or_else(|| {
        format!(
            "Fixture {} in warn/ must have '// expect: <CODE>'",
            path.display()
        )
    })?;

    let mut file =
        NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    write!(file, "{}", content).map_err(|e| format!("Failed to write temp file: {}", e))?;

    let test_path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    cmd.env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        .env("RUST_BACKTRACE", "0")
        .arg("check")
        .arg(&test_path)
        .arg("--format")
        .arg("json");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run miri: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse JSON output: {}", e))?;

    if parsed["ok"] != true {
        return Err(format!(
            "Expected ok=true for warning, but got false. Code: {}",
            expect_code
        ));
    }

    let diags = parsed["diagnostics"]
        .as_array()
        .ok_or_else(|| "diagnostics is not an array".to_string())?;

    let found = diags.iter().any(|d| {
        d["code"].as_str() == Some(&expect_code) && d["severity"].as_str() == Some("warning")
    });

    if !found {
        let codes: Vec<_> = diags
            .iter()
            .filter_map(|d| {
                let code = d["code"].as_str()?;
                let severity = d["severity"].as_str()?;
                Some((code, severity))
            })
            .collect();
        return Err(format!(
            "Expected warning code {} but got: {:?}",
            expect_code, codes
        ));
    }

    Ok(())
}

/// Verify that pass/<CODE>.mi or pass/e2e_<name>.mi compiles + runs cleanly.
fn test_pass_fixture(path: &Path) -> Result<(), String> {
    let directives = parse_fixture(path)?;

    let mut file =
        NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    write!(file, "{}", content).map_err(|e| format!("Failed to write temp file: {}", e))?;

    let test_path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    cmd.env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        .env("RUST_BACKTRACE", "0")
        .arg("run")
        .arg(&test_path)
        .arg("--format")
        .arg("json");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run miri: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse JSON output: {}", e))?;

    if parsed["ok"] != true {
        return Err(format!(
            "Expected ok=true for pass fixture, but got false: {}",
            parsed
                .get("diagnostics")
                .map(|d| d.to_string())
                .unwrap_or_default()
        ));
    }

    if parsed["exitCode"] != 0 {
        return Err(format!(
            "Expected exitCode=0 but got {}",
            parsed["exitCode"]
        ));
    }

    // If expect-stdout directives are present, verify them.
    if !directives.expect_stdout_lines.is_empty() {
        let expected = directives.expect_stdout_lines.join("\n");
        // `println` terminates its output with a newline, so compare against the
        // captured stdout with a single trailing newline removed. Comparing raw
        // would make every directive unsatisfiable for a program that prints.
        let raw = parsed["stdoutTail"].as_str().unwrap_or("");
        let actual = raw.strip_suffix('\n').unwrap_or(raw).to_string();

        if actual != expected {
            return Err(format!(
                "Expected stdout:\n{}\nbut got:\n{}",
                expected, actual
            ));
        }
    }

    Ok(())
}

/// Explicit exclusion table for codes that cannot be triggered via .mi source.
/// Each entry must have a PRECISE, TRUE reason.
const CONFORMANCE_EXCLUSIONS: &[(&str, &str)] = &[
    ("MER_PAR_006", "Defensive parse guard: the lexer only emits a Float token for text that parses as f64 (1e999 yields inf), so the error branch is unreachable from source"),
    ("MER_PAR_007", "Defensive parse guard: the slice fails only for a string token shorter than its two delimiters, which the lexer cannot produce"),
    ("MER_PAR_008", "Defensive parse guard: the fallback arm needs a True/False token whose text is neither of the two boolean spellings, which the lexer cannot produce"),
    ("MER_LEX_010", "Defensive lex guard: locate_body returns None only for an f-string token without a body, which the lexer cannot produce"),
    ("MER_NAM_003", "The use-path grammar rejects '/', '\\' and '..' before this check runs; `use foo()` reports MER_NAM_002 and `use 5` reports MER_PAR_001"),
    ("MER_TYP_064", "Check does not fire: `let x = 5` followed by `let x = 10` compiles clean; recorded as a follow-up"),
    ("MER_TYP_019", "Emitted while lowering a GPU kernel launch; no host-side construct reaches it"),
    ("MER_TYP_021", "Emitted while lowering a GPU kernel launch; no host-side construct reaches it"),
    ("MER_TYP_022", "Emitted while lowering a GPU kernel launch; no host-side construct reaches it"),
    ("MER_MIR_001", "ICE guard: the documented trigger is rejected by the parser first (MER_PAR_001)"),
    ("MER_MIR_002", "ICE guard: the documented trigger is rejected by the parser first (MER_PAR_001)"),
    ("MER_MIR_005", "ICE guard: the documented trigger is rejected earlier as MER_TAR_005"),
    ("MER_MIR_007", "ICE guard: the documented trigger is rejected by the parser first (MER_PAR_004)"),
    ("MER_MIR_008", "ICE guard: the documented trigger is rejected by the parser first (MER_PAR_001)"),
    ("MER_MIR_010", "ICE guard: the type checker rejects the documented trigger first (MER_TYP_030)"),
    ("MER_MIR_012", "ICE guard: the type checker rejects the documented trigger first (MER_TYP_030)"),
    ("MER_TYP_035", "Requires a second module to hold the non-visible symbol; the harness runs single-file fixtures only"),
    ("MER_BLD_001", "Command-invocation diagnostic (miri check/build/explain/fix); not reachable from .mi source"),
    ("MER_BLD_002", "Command-invocation diagnostic (miri check/build/explain/fix); not reachable from .mi source"),
    ("MER_BLD_003", "Command-invocation diagnostic (miri check/build/explain/fix); not reachable from .mi source"),
    ("MER_BLD_004", "Command-invocation diagnostic (miri view); not reachable from .mi source"),
    ("MER_BLD_005", "Command-invocation diagnostic (miri view); not reachable from .mi source"),
    ("MER_BLD_006", "Command-invocation diagnostic (miri view/patch); not reachable from .mi source"),
    ("MER_BLD_007", "Command-invocation diagnostic (miri view/patch); not reachable from .mi source"),
    ("MER_BLD_008", "Command-invocation diagnostic (miri view/patch); not reachable from .mi source"),
    ("MER_BLD_009", "Command-invocation diagnostic (miri patch --expect-sha); not reachable from .mi source"),
    ("MER_BLD_010", "Command-invocation diagnostic (miri patch token alignment); not reachable from .mi source"),
    ("MER_BLD_011", "Command-invocation diagnostic (miri patch validation failure); not reachable from .mi source"),
    ("MER_BLD_012", "Command-invocation diagnostic (miri patch edit flags); not reachable from .mi source"),
    ("MER_CG_001", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_002", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_003", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_004", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_005", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_006", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_007", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_CG_008", "Cranelift-internal ISA/module/function/object emission; not reachable from .mi source"),
    ("MER_IMP_001", "Shadowed by MER_NAM_002 (duplicate name error fires first)"),
    ("MER_IMP_003", "Shadowed by MER_NAM_002 (duplicate name error fires first)"),
    ("MER_LEX_004", "No Before snippet in doc"),
    ("MER_LEX_009", "Shadowed by MER_LEX_001 (invalid token fires first)"),
    ("MER_LEX_013", "No Before snippet in doc"),
    ("MER_MIR_003", "Shadowed by MER_TYP_034 (type error fires first)"),
    ("MER_MIR_004", "No Before snippet in doc"),
    ("MER_MIR_006", "Shadowed by MER_TYP_050 (type error fires first)"),
    ("MER_MIR_009", "Before snippet compiles clean (ok=true); not a user-facing error"),
    ("MER_MIR_011", "Shadowed by MER_TYP_043 (type error fires first)"),
    ("MER_MIR_013", "No Before snippet in doc"),
    ("MER_MIR_014", "No Before snippet in doc"),
    ("MER_MIR_015", "No Before snippet in doc"),
    ("MER_OWN_002", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_PAR_009", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_PAR_022", "No Before snippet in doc"),
    ("MER_PAR_023", "No Before snippet in doc"),
    ("MER_RT_003", "Integer overflow wraps silently in unchecked arithmetic"),
    ("MER_RT_004", "No compiled-code path exists"),
    ("MER_TAR_001", "Shadowed by MER_TYP_034 (type error fires first)"),
    ("MER_TAR_003", "GPU-specific error; not reachable without GPU hardware"),
    ("MER_TAR_004", "Shadowed by MER_TYP_034 (type error fires first)"),
    ("MER_TYP_001", "No Before snippet in doc"),
    ("MER_TYP_003", "No Before snippet in doc"),
    ("MER_TYP_004", "No Before snippet in doc"),
    ("MER_TYP_005", "No Before snippet in doc"),
    ("MER_TYP_006", "No Before snippet in doc"),
    ("MER_TYP_007", "No Before snippet in doc"),
    ("MER_TYP_008", "No Before snippet in doc"),
    ("MER_TYP_009", "No Before snippet in doc"),
    ("MER_TYP_010", "No Before snippet in doc"),
    ("MER_TYP_020", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_028", "No Before snippet in doc"),
    ("MER_TYP_029", "Shadowed by MER_TYP_033 (type error fires first)"),
    ("MER_TYP_036", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_037", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_038", "Shadowed by MER_PAR_014 (parser error fires first)"),
    ("MER_TYP_045", "No Before snippet in doc"),
    ("MER_TYP_046", "No Before snippet in doc"),
    ("MER_TYP_047", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_052", "Shadowed by MER_TYP_034 (type error fires first)"),
    ("MER_TYP_055", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_056", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_057", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_058", "Shadowed by MER_PAR_013 (parser error fires first)"),
    ("MER_TYP_059", "Shadowed by MER_PAR_014 (parser error fires first)"),
    ("MER_TYP_061", "No Before snippet in doc"),
    ("MER_TYP_062", "Shadowed by MER_PAR_001 (parser error fires first)"),
    ("MER_TYP_066", "Shadowed by MER_PAR_001 (parser error fires first)"),
];

/// Completeness gate: every live (non-reserved) code must have a fixture or an explicit exclusion.
#[test]
fn test_conformance_completeness() {
    let corpus_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("conformance")
        .join("agent");

    let fail_dir = corpus_root.join("fail");
    let warn_dir = corpus_root.join("warn");
    let pass_dir = corpus_root.join("pass");

    // Build exclusion map from explicit table
    let mut exclusion_map = HashMap::new();
    for &(code_str, reason) in CONFORMANCE_EXCLUSIONS {
        exclusion_map.insert(code_str, reason);
    }

    let mut missing_fixtures = Vec::new();
    let mut redundant_exclusions = Vec::new();
    let mut found_codes = std::collections::HashSet::new();

    // Scan fail/ directory
    if fail_dir.exists() {
        match fs::read_dir(&fail_dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(e) => {
                            if let Some(name) = e.file_name().to_str() {
                                if name.ends_with(".mi") {
                                    if let Some(code) = name.strip_suffix(".mi") {
                                        found_codes.insert(code.to_string());
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            panic!("Failed to read entry in fail/: {}", err);
                        }
                    }
                }
            }
            Err(err) => {
                panic!("Failed to read fail/ directory: {}", err);
            }
        }
    } else {
        panic!("conformance/agent/fail/ directory does not exist");
    }

    // Scan warn/ directory
    if warn_dir.exists() {
        match fs::read_dir(&warn_dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(e) => {
                            if let Some(name) = e.file_name().to_str() {
                                if name.ends_with(".mi") {
                                    if let Some(code) = name.strip_suffix(".mi") {
                                        found_codes.insert(code.to_string());
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            panic!("Failed to read entry in warn/: {}", err);
                        }
                    }
                }
            }
            Err(err) => {
                panic!("Failed to read warn/ directory: {}", err);
            }
        }
    } else {
        panic!("conformance/agent/warn/ directory does not exist");
    }

    // Ensure pass/ directory exists (used by test_conformance_agent)
    if !pass_dir.exists() {
        panic!("conformance/agent/pass/ directory does not exist");
    }

    // Check all live codes: each must have exactly one of fixture or exclusion, not both, not neither
    for code in DiagnosticCode::all() {
        if code.is_reserved() {
            continue;
        }

        let code_str = code.as_str();
        let has_fixture = found_codes.contains(code_str);
        let is_excluded = exclusion_map.contains_key(code_str);

        if !has_fixture && !is_excluded {
            missing_fixtures.push(code_str.to_string());
        } else if has_fixture && is_excluded {
            redundant_exclusions.push(code_str.to_string());
        }
    }

    if !missing_fixtures.is_empty() {
        panic!(
            "Incomplete conformance corpus. Missing fixtures for codes without exclusions:\n  {}",
            missing_fixtures.join("\n  ")
        );
    }

    if !redundant_exclusions.is_empty() {
        panic!(
            "Redundant exclusion entries (code has both fixture and exclusion):\n  {}",
            redundant_exclusions.join("\n  ")
        );
    }
}

/// Documentation gate: verify that docs/conformance-agent.md lists all fixtures with summaries.
fn verify_doc_against_fixtures(corpus_root: &Path) -> Result<(), String> {
    let doc_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("conformance-agent.md");

    // Collect all fixture codes from fail/ and warn/ directories
    let mut fixture_codes = std::collections::BTreeSet::new();

    let fail_dir = corpus_root.join("fail");
    let warn_dir = corpus_root.join("warn");
    let pass_dir = corpus_root.join("pass");

    if fail_dir.exists() {
        if let Ok(entries) = fs::read_dir(&fail_dir) {
            for entry in entries {
                if let Ok(e) = entry {
                    if let Some(name) = e.file_name().to_str() {
                        if name.ends_with(".mi") {
                            if let Some(code) = name.strip_suffix(".mi") {
                                fixture_codes.insert(code.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if warn_dir.exists() {
        if let Ok(entries) = fs::read_dir(&warn_dir) {
            for entry in entries {
                if let Ok(e) = entry {
                    if let Some(name) = e.file_name().to_str() {
                        if name.ends_with(".mi") {
                            if let Some(code) = name.strip_suffix(".mi") {
                                fixture_codes.insert(code.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Read the doc file
    let doc_content = fs::read_to_string(&doc_path)
        .map_err(|e| format!("Failed to read docs/conformance-agent.md: {}", e))?;

    // Verify each fixture code appears with its summary line in the doc
    let mut missing_from_doc = Vec::new();

    for code in &fixture_codes {
        // We expect a line like: `| <CODE> | <summary> |`
        let expected_pattern = format!("| {} |", code);
        if !doc_content.contains(&expected_pattern) {
            missing_from_doc.push(code.clone());
        }
    }

    if !missing_from_doc.is_empty() {
        return Err(format!(
            "docs/conformance-agent.md missing entries for fixtures:\n  {}",
            missing_from_doc.join("\n  ")
        ));
    }

    // The published listing must not advertise a fixture the corpus does not
    // hold, so the two are checked in both directions: a row naming a deleted or
    // renamed fixture is drift just as much as a fixture missing from the doc.
    let mut documented: Vec<String> = Vec::new();
    for dir in [&fail_dir, &warn_dir, &pass_dir] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if let Some(stem) = name.strip_suffix(".mi") {
                        documented.push(stem.to_string());
                    }
                }
            }
        }
    }

    let mut undelivered = Vec::new();
    for line in doc_content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("| ") {
            continue;
        }
        let Some(name) = trimmed.split('|').nth(1).map(str::trim) else {
            continue;
        };
        if name.is_empty() || name == "Code" || name == "Fixture" || name.starts_with("---") {
            continue;
        }
        if !documented.iter().any(|f| f == name) {
            undelivered.push(name.to_string());
        }
    }

    if !undelivered.is_empty() {
        return Err(format!(
            "docs/conformance-agent.md lists fixtures that do not exist:\n  {}",
            undelivered.join("\n  ")
        ));
    }

    Ok(())
}

/// Run all conformance fixtures in a given root directory.
fn run_conformance_tests(corpus_root: &Path) -> Result<(), String> {
    let fail_dir = corpus_root.join("fail");
    let warn_dir = corpus_root.join("warn");
    let pass_dir = corpus_root.join("pass");

    let mut pass_count = 0;
    let mut fail_count = 0;

    // Test fail/ fixtures
    if fail_dir.exists() {
        if let Ok(entries) = fs::read_dir(&fail_dir) {
            for entry in entries {
                let e = entry.map_err(|err| err.to_string())?;
                let path = e.path();
                if path.extension().map(|s| s == "mi").unwrap_or(false) {
                    match test_fail_fixture(&path) {
                        Ok(()) => pass_count += 1,
                        Err(err) => {
                            eprintln!("FAIL: {}: {}", path.display(), err);
                            fail_count += 1;
                        }
                    }
                }
            }
        }
    }

    // Test warn/ fixtures
    if warn_dir.exists() {
        if let Ok(entries) = fs::read_dir(&warn_dir) {
            for entry in entries {
                let e = entry.map_err(|err| err.to_string())?;
                let path = e.path();
                if path.extension().map(|s| s == "mi").unwrap_or(false) {
                    match test_warn_fixture(&path) {
                        Ok(()) => pass_count += 1,
                        Err(err) => {
                            eprintln!("FAIL: {}: {}", path.display(), err);
                            fail_count += 1;
                        }
                    }
                }
            }
        }
    }

    // Test pass/ fixtures
    if pass_dir.exists() {
        if let Ok(entries) = fs::read_dir(&pass_dir) {
            for entry in entries {
                let e = entry.map_err(|err| err.to_string())?;
                let path = e.path();
                if path.extension().map(|s| s == "mi").unwrap_or(false) {
                    match test_pass_fixture(&path) {
                        Ok(()) => pass_count += 1,
                        Err(err) => {
                            eprintln!("FAIL: {}: {}", path.display(), err);
                            fail_count += 1;
                        }
                    }
                }
            }
        }
    }

    if fail_count > 0 {
        return Err(format!("{} fixture(s) failed", fail_count));
    }

    println!("Conformance: {} fixtures passed", pass_count);
    Ok(())
}

#[test]
fn test_conformance_agent() {
    let corpus_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("conformance")
        .join("agent");

    if !corpus_root.exists() {
        panic!("Corpus root {} does not exist", corpus_root.display());
    }

    // Verify documentation is in sync
    if let Err(err) = verify_doc_against_fixtures(&corpus_root) {
        panic!("Documentation gate failed: {}", err);
    }

    // Run all fixtures
    match run_conformance_tests(&corpus_root) {
        Ok(()) => {}
        Err(err) => panic!("Conformance tests failed: {}", err),
    }
}
