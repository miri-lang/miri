use assert_cmd::{pkg_name, Command};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use tempfile::NamedTempFile;

pub const BINARY_NAME: &str = pkg_name!();
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn miri_cmd() -> Command {
    Command::new(assert_cmd::cargo_bin!("miri"))
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
    let output = cmd
        .env("RUST_BACKTRACE", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .arg(command)
        .arg(&path)
        .output()
        .unwrap();

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

/// Run a multi-file Miri project.
///
/// `files` is a slice of `(relative_path, content)` pairs. The first file is
/// used as the entry point (`miri run <first_path>`). All files are written
/// into a temporary directory and the compiler is invoked with that directory
/// as the working directory. `MIRI_STDLIB_PATH` is set to the project's own
/// stdlib so it remains accessible even when CWD changes.
pub fn miri_run_project(files: &[(&str, &str)]) -> CompilerResult {
    use std::fs;
    use tempfile::tempdir;

    assert!(
        !files.is_empty(),
        "miri_run_project: files list must not be empty"
    );

    let temp_dir = tempdir().unwrap();

    for (rel_path, content) in files {
        let file_path = temp_dir.path().join(rel_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(file_path, content).unwrap();
    }

    let entry_file = files[0].0;
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let mut cmd = miri_cmd();
    let output = cmd
        .env("RUST_BACKTRACE", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        // Prevent linker-override env vars from leaking in from concurrent tests.
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .current_dir(temp_dir.path())
        .arg("run")
        .arg(entry_file)
        .output()
        .unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Strip ANSI escape codes from a string
pub fn strip_ansi(s: &str) -> String {
    static ANSI_RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = ANSI_RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap());
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
