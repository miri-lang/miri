// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

const TYPE_ERROR_CODE: &str = r#"fn main() int
    if 42
        100
    200
"#;

#[test]
fn test_json_format_with_color_always_no_ansi() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .arg("--color")
        .arg("always")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("\x1b"),
        "JSON output must never contain ANSI escape codes, even with --color always"
    );
}

#[test]
fn test_color_always_with_format_pretty_has_ansi() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("pretty")
        .arg("--color")
        .arg("always")
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("\x1b"),
        "Pretty format with --color always should contain ANSI escape codes in stderr"
    );
}

#[test]
fn test_color_never_with_format_pretty_no_ansi() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("pretty")
        .arg("--color")
        .arg("never")
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("\x1b"),
        "Pretty format with --color never should not contain ANSI escape codes"
    );
}

#[test]
fn test_color_auto_respects_tty() {
    // This test verifies that --color auto is a valid argument
    // The actual TTY detection behavior depends on the environment
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--color")
        .arg("auto")
        .output()
        .unwrap();

    // Should exit with code 1 due to the type error, not a CLI arg parsing error
    assert_eq!(
        output.status.code(),
        Some(1),
        "Should exit with 1 for type error, not a CLI parsing error"
    );
}
