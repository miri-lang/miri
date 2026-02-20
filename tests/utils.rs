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

/// Execute Miri binary with given command
fn exec_miri(command: &str, input: &str) -> CompilerResult {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", input).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let output = cmd.arg(command).arg(&path).output().unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Run Miri binary with 'check' command (type-checking only)
pub fn miri_check(input: &str) -> CompilerResult {
    exec_miri("check", input)
}

/// Run Miri binary with 'build' command (compilation)
pub fn miri_build(input: &str) -> CompilerResult {
    exec_miri("build", input)
}

/// Run Miri binary with 'run' command (compilation + execution)
pub fn miri_run(input: &str) -> CompilerResult {
    exec_miri("run", input)
}

/// Strip ANSI escape codes from a string
pub fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

/// Check that the output contains the expected error messages
pub fn check_error_output(source: &str, expected_parts: &[&str]) {
    let result = miri_check(source);
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
