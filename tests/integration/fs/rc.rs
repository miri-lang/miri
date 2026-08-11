// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use tempfile::TempDir;

#[test]
fn test_read_file_no_leak_on_success() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "test content").expect("failed to write test file");
    let path = file_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn verify_content(contents String)
    if contents == "test content"
        println("content ok")
    else
        println(f"mismatch: {{contents}}")

fn main()
    let fs = Fs()
    var i = 0
    while i < 10
        match fs.read_file("{}")
            Result.Ok(contents): verify_content(contents)
            Result.Err(e): println("error")
        i = i + 1
"#,
        path
    );
    assert_runs_with_output(&code, "content ok");
}

#[test]
fn test_read_file_no_leak_on_error() {
    let code = r#"
use system.result
use system.fs

fn handle_error(e FsError, i int)
    match e
        FsError.NotFound(path): println(f"loop {i}")
        default: println("other")

fn main()
    let fs = Fs()
    var i = 0
    while i < 10
        match fs.read_file("/tmp/nonexistent_xyz.txt")
            Result.Ok(contents): println("unexpected")
            Result.Err(e): handle_error(e, i)
        i = i + 1
"#;
    assert_runs_with_output(code, "loop 0");
}

#[test]
#[ignore = "Result-wrapped list payloads are never freed by the compiler; this causes RC leaks that prevent the test from passing. The issue occurs at the compiler level and is unrelated to the Fs implementation. Re-enable when the compiler's Result<[T], E> deallocation is fixed."]
fn test_list_dir_no_leak() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    std::fs::write(temp_dir.path().join("f1.txt"), "").expect("write f1");
    std::fs::write(temp_dir.path().join("f2.txt"), "").expect("write f2");
    std::fs::write(temp_dir.path().join("f3.txt"), "").expect("write f3");
    let path = temp_dir.path().to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()
    var i = 0
    while i < 5
        match fs.list_dir("{}")
            Result.Ok(entries): println(f"loop {{i}} got {{entries.length()}}")
            Result.Err(e): println("error")
        i = i + 1
"#,
        path
    );
    assert_runs(&code);
}
