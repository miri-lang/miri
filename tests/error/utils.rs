// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use assert_cmd::Command;
use std::io::Write;
use tempfile::NamedTempFile;

pub fn run_compiler_with_input(input: &str) -> String {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", input).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let mut cmd = Command::cargo_bin("miri").unwrap();
    let output = cmd.arg("run").arg(&path).output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    format!("{}{}", stdout, stderr)
}

pub fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

pub fn check_error_output(source: &str, expected_parts: &[&str]) {
    let output = run_compiler_with_input(source);
    let clean_output = strip_ansi(&output);

    for part in expected_parts {
        assert!(
            clean_output.contains(part),
            "Output did not contain expected part.\nExpected: '{}'\nActual Output:\n{}",
            part,
            clean_output
        );
    }
}
