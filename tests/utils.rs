use assert_cmd::{pkg_name, Command};
use std::io::Write;
use tempfile::NamedTempFile;

pub const BINARY_NAME: &str = pkg_name!();
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[allow(deprecated)]
pub fn miri_cmd() -> Command {
    Command::cargo_bin(BINARY_NAME).unwrap()
}

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

    // Use 'check' command for type-checking only (no codegen)
    let mut cmd = miri_cmd();
    let output = cmd.arg("check").arg(&path).output().unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

pub fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

pub fn check_error_output(source: &str, expected_parts: &[&str]) {
    let result = run_compiler(source);
    let output = result.output();
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
