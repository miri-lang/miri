// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use miri::cli::determinism::{compare_snapshots, drift_diagnostic, Drift, DriftKind};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

const HELLO_WORLD: &str = r#"
use system.io

fn main()
    println("Hello, World!")
"#;

const MULTI_LITERAL: &str = r#"
use system.io

fn main()
    println("First string")
    println("Second string")
    println("Third string")
"#;

#[test]
fn test_determinism_check_passes_valid_file() {
    let file = create_test_file(HELLO_WORLD);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("determinism")
        .arg("check")
        .arg(path)
        .assert()
        .success()
        .stdout(predicates::str::contains("Determinism check passed"));
}

#[test]
fn test_determinism_check_with_multiple_strings() {
    let file = create_test_file(MULTI_LITERAL);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("determinism")
        .arg("check")
        .arg(path)
        .assert()
        .success()
        .stdout(predicates::str::contains("Determinism check passed"));
}

#[test]
fn test_determinism_check_json_format_pass() {
    let file = create_test_file(HELLO_WORLD);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("determinism")
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON");

    // Verify the full envelope shape
    assert_eq!(json["schemaVersion"], 1, "schemaVersion should be 1");
    assert_eq!(
        json["ok"], true,
        "ok should be true for deterministic artifacts"
    );
    assert_eq!(
        json["command"], "determinism",
        "command should be 'determinism'"
    );
    assert_eq!(
        json["diagnostics"],
        serde_json::Value::Array(vec![]),
        "diagnostics should be empty for passing check"
    );
    assert_eq!(json["exitCode"], 0, "exitCode should be 0 for success");
}

#[test]
fn test_determinism_check_build_failure() {
    // Create a file that does not compile.
    let invalid_file = create_test_file("this is not valid miri code");
    let path = invalid_file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("determinism")
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON");

    // Should be a build failure (compiler error), not a determinism drift
    assert_eq!(json["ok"], false);
    assert!(json["diagnostics"].is_array());
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert!(
        !diagnostics.is_empty(),
        "build failure should have diagnostics"
    );

    // MER_BLD_003 should NOT be present (it's only for drift)
    let has_drift_code = diagnostics
        .iter()
        .any(|d| d["code"].as_str() == Some("MER_BLD_003"));
    assert!(
        !has_drift_code,
        "build failure should not have MER_BLD_003 code"
    );
}

#[test]
fn test_determinism_check_help() {
    let mut cmd = miri_cmd();
    cmd.arg("determinism")
        .arg("check")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Check if an input builds deterministically",
        ));
}

#[test]
fn test_determinism_check_missing_file() {
    let mut cmd = miri_cmd();
    cmd.arg("determinism")
        .arg("check")
        .arg("/nonexistent/path/to/file.mi")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("could not read"));
}

// NOTE: Web-GPU bundle determinism test is blocked on fixing a pre-existing
// non-determinism bug in the WGSL compilation or buffer ordering in
// src/codegen/web_gpu/mod.rs. The test infrastructure is in place; just
// uncomment below once web-gpu bundles compile deterministically.
//
// #[test]
// fn test_determinism_check_web_gpu_bundle() {
//     let file = create_test_file(GPU_SIMPLE);
//     let path = file.path().to_str().unwrap();
//     let mut cmd = miri_cmd();
//     cmd.arg("determinism")
//         .arg("check")
//         .arg(path)
//         .arg("--target")
//         .arg("web-gpu")
//         .assert()
//         .success()
//         .stdout(predicates::str::contains("Determinism check passed"));
// }

#[test]
fn test_determinism_check_build_failure_has_diagnostics() {
    // Create a file that does not compile (type error).
    let invalid_file = create_test_file("var x: int = \"not an int\"");
    let path = invalid_file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    let output = cmd
        .arg("determinism")
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON");

    // Verify ok is false and diagnostics have compiler errors
    assert_eq!(json["ok"], false);
    assert!(json["diagnostics"].is_array());
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert!(
        !diagnostics.is_empty(),
        "build failure should have diagnostics"
    );

    // At least one diagnostic should have a code (compiler error).
    let has_code = diagnostics.iter().any(|d| d["code"].is_string());
    assert!(
        has_code,
        "Expected at least one diagnostic with a code field"
    );

    // MER_BLD_003 should NOT be present (it's only for drift)
    let has_drift_code = diagnostics
        .iter()
        .any(|d| d["code"].as_str() == Some("MER_BLD_003"));
    assert!(
        !has_drift_code,
        "build failure should not have MER_BLD_003 code (that's only for drift)"
    );
}

#[cfg(test)]
mod drift_detection {
    use super::*;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn test_identical_trees_no_drift() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.txt"), b"content".to_vec());
        run1.insert(path("dir/file.txt"), b"nested".to_vec());

        let run2 = run1.clone();

        let drifts = compare_snapshots(&run1, &run2);
        assert!(drifts.is_empty());
    }

    #[test]
    fn test_bytes_differ_same_length() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.bin"), b"0123456789ABCDEF".to_vec());

        let mut run2 = BTreeMap::new();
        // Differ at offset 7: 'G' instead of '7'
        run2.insert(path("file.bin"), b"0123456G89ABCDEF".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        assert_eq!(drift.path, path("file.bin"));

        match &drift.kind {
            DriftKind::BytesMismatch {
                offset,
                hex_run1,
                hex_run2,
            } => {
                assert_eq!(*offset, 7);
                assert!(!hex_run1.is_empty());
                assert!(!hex_run2.is_empty());
                // hex_run1 should contain the bytes around offset 7 (starting around offset 3)
                // The window is 16 bytes, starting at offset 3 (7 - 16/2)
                // So we get bytes 3-18, but file is only 16 bytes, so bytes 3-15
                // hex run1: "3456789ABCDEF"
                // hex run1 should contain '37' (ASCII '7')
                assert!(hex_run1.contains("37"), "expected '37' (ASCII '7') in hex");
                // hex_run2 should have the same window but with 'G' (47) instead of '7' (37)
                assert!(hex_run2.contains("47"), "expected '47' (ASCII 'G') in hex");
            }
            _ => panic!("expected BytesMismatch variant"),
        }
    }

    #[test]
    fn test_length_mismatch() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.bin"), b"short".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file.bin"), b"much longer content".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        assert_eq!(drift.path, path("file.bin"));

        match &drift.kind {
            DriftKind::LengthMismatch { len_run1, len_run2 } => {
                assert_eq!(*len_run1, 5);
                assert_eq!(*len_run2, 19);
            }
            _ => panic!("expected LengthMismatch variant"),
        }
    }

    #[test]
    fn test_path_present_in_run1_only() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file1.txt"), b"content1".to_vec());
        run1.insert(path("file2.txt"), b"content2".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file1.txt"), b"content1".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        assert_eq!(drift.path, path("file2.txt"));
        assert_eq!(drift.kind, DriftKind::PresentInRun1Only);
    }

    #[test]
    fn test_path_present_in_run2_only() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file1.txt"), b"content1".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file1.txt"), b"content1".to_vec());
        run2.insert(path("file2.txt"), b"content2".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        assert_eq!(drift.path, path("file2.txt"));
        assert_eq!(drift.kind, DriftKind::PresentInRun2Only);
    }

    #[test]
    fn test_multiple_drifts() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file1.txt"), b"content1".to_vec());
        run1.insert(path("file2.txt"), b"content2".to_vec());
        run1.insert(path("file3.txt"), b"content3".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file1.txt"), b"CHANGED1".to_vec()); // Different content
        run2.insert(path("file2.txt"), b"content2".to_vec()); // Same
                                                              // file3.txt missing in run2

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 2);

        let drift_file1 = drifts.iter().find(|d| d.path == path("file1.txt")).unwrap();
        assert!(matches!(drift_file1.kind, DriftKind::BytesMismatch { .. }));

        let drift_file3 = drifts.iter().find(|d| d.path == path("file3.txt")).unwrap();
        assert_eq!(drift_file3.kind, DriftKind::PresentInRun1Only);
    }

    #[test]
    fn test_nested_path_drift() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("dir1/dir2/file.txt"), b"v1".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("dir1/dir2/file.txt"), b"v2".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].path, path("dir1/dir2/file.txt"));
    }

    #[test]
    fn test_bytes_differ_at_offset_zero() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.bin"), b"ABCDEF".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file.bin"), b"XBCDEF".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        match &drift.kind {
            DriftKind::BytesMismatch { offset, .. } => {
                assert_eq!(*offset, 0, "drift should be at offset 0");
            }
            _ => panic!("expected BytesMismatch at offset 0"),
        }
    }

    #[test]
    fn test_bytes_differ_at_final_byte() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.bin"), b"ABCDEF".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file.bin"), b"ABCDEX".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        match &drift.kind {
            DriftKind::BytesMismatch { offset, .. } => {
                assert_eq!(*offset, 5, "drift should be at offset 5 (final byte)");
            }
            _ => panic!("expected BytesMismatch at final byte"),
        }
    }

    #[test]
    fn test_zero_byte_artifact() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.bin"), b"".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file.bin"), b"".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert!(drifts.is_empty(), "zero-byte artifacts should match");
    }

    #[test]
    fn test_zero_byte_vs_nonempty() {
        let mut run1 = BTreeMap::new();
        run1.insert(path("file.bin"), b"".to_vec());

        let mut run2 = BTreeMap::new();
        run2.insert(path("file.bin"), b"content".to_vec());

        let drifts = compare_snapshots(&run1, &run2);
        assert_eq!(drifts.len(), 1);

        let drift = &drifts[0];
        match &drift.kind {
            DriftKind::LengthMismatch { len_run1, len_run2 } => {
                assert_eq!(*len_run1, 0);
                assert_eq!(*len_run2, 7);
            }
            _ => panic!("expected LengthMismatch"),
        }
    }
}

#[cfg(test)]
mod drift_diagnostic_mapping {
    use super::*;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn test_bytes_mismatch_diagnostic() {
        let drift = Drift {
            path: path("artifact.bin"),
            kind: DriftKind::BytesMismatch {
                offset: 42,
                hex_run1: "41 42 43".to_string(),
                hex_run2: "41 42 44".to_string(),
            },
        };

        let diag = drift_diagnostic(&drift);

        assert_eq!(diag.code, Some("MER_BLD_003".to_string()));
        assert_eq!(diag.path, Some("artifact.bin".to_string()));
        assert_eq!(diag.severity, "error");
        assert!(
            diag.message.contains("42"),
            "message should contain offset: {}",
            diag.message
        );
        assert!(
            diag.help.as_ref().unwrap().contains("41 42 43"),
            "help should contain run1 hex window"
        );
        assert!(
            diag.help.as_ref().unwrap().contains("41 42 44"),
            "help should contain run2 hex window"
        );
    }

    #[test]
    fn test_length_mismatch_diagnostic() {
        let drift = Drift {
            path: path("artifact.bin"),
            kind: DriftKind::LengthMismatch {
                len_run1: 100,
                len_run2: 200,
            },
        };

        let diag = drift_diagnostic(&drift);

        assert_eq!(diag.code, Some("MER_BLD_003".to_string()));
        assert_eq!(diag.path, Some("artifact.bin".to_string()));
        assert_eq!(diag.severity, "error");
        assert!(diag.message.contains("100") && diag.message.contains("200"));
        assert!(diag.help.is_some());
    }

    #[test]
    fn test_present_in_run1_only_diagnostic() {
        let drift = Drift {
            path: path("extra_file.o"),
            kind: DriftKind::PresentInRun1Only,
        };

        let diag = drift_diagnostic(&drift);

        assert_eq!(diag.code, Some("MER_BLD_003".to_string()));
        assert_eq!(diag.path, Some("extra_file.o".to_string()));
        assert_eq!(diag.severity, "error");
        assert!(diag.message.contains("run 1"));
        assert!(diag.help.is_some());
    }

    #[test]
    fn test_present_in_run2_only_diagnostic() {
        let drift = Drift {
            path: path("extra_file.o"),
            kind: DriftKind::PresentInRun2Only,
        };

        let diag = drift_diagnostic(&drift);

        assert_eq!(diag.code, Some("MER_BLD_003".to_string()));
        assert_eq!(diag.path, Some("extra_file.o".to_string()));
        assert_eq!(diag.severity, "error");
        assert!(diag.message.contains("run 2"));
        assert!(diag.help.is_some());
    }
}
