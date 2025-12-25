// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use assert_cmd::Command;
use std::io::Write;
use tempfile::NamedTempFile;

pub struct CompilerResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CompilerResult {
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

pub fn run_compiler(input: &str) -> CompilerResult {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", input).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let mut cmd = Command::cargo_bin("miri").unwrap();

    // For now, we capture only output and status.
    let output = cmd.arg("run").arg(&path).output().unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

pub fn assert_valid(code: &str) {
    let result = run_compiler(code);

    // We check if there are any "error:" lines in the output.
    let output = result.output();
    if output.contains("error:") {
        panic!("Expected valid program, but got errors:\n{}", output);
    }
}

pub fn assert_invalid(code: &str, expected_errors: &[&str]) {
    let result = run_compiler(code);
    let output = result.output();

    if !output.contains("error:") {
        panic!(
            "Expected invalid program, but got no errors.\nOutput:\n{}",
            output
        );
    }

    for error in expected_errors {
        if !output.contains(error) {
            panic!(
                "Expected error '{}' not found in output:\n{}",
                error, output
            );
        }
    }
}
