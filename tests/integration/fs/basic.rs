// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::temp_test_file;
use super::utils::*;
use crate::utils::miri_run;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_fs_class_instantiation() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()
    println("fs created")
"#,
        "fs created",
    );
}

#[test]
fn test_exists_returns_false_for_missing_file() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()
    if fs.exists("/tmp/missing_file_that_should_not_exist_12345.txt")
        println("missing exists: yes")
    else
        println("missing exists: no")
"#,
        "missing exists: no",
    );
}

#[test]
fn test_exists_returns_true_for_existing_file() {
    let (_temp, path) = temp_test_file("test_exists.txt");
    std::fs::write(&path, "content").expect("failed to write test file");

    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()
    if fs.exists("{}")
        println("file exists: yes")
    else
        println("file exists: no")
"#,
        path
    );
    assert_runs_with_output(&code, "file exists: yes");
}

#[test]
fn test_write_and_read_file() {
    let (_temp, path) = temp_test_file("test_write_read.txt");
    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()
    let temp_path = "{}"

    match fs.write_file(temp_path, "Hello, World!")
        Result.Ok(n): println(f"write: {{n}} bytes")
        Result.Err(e): println("write: error")

    match fs.read_file(temp_path)
        Result.Ok(contents): print_read_result(contents)
        Result.Err(e): println("read: error")

fn print_read_result(contents String)
    if contents == "Hello, World!"
        println("read: ok")
    else
        println(f"read: mismatch got {{contents}}")
"#,
        path
    );
    let result = miri_run(&code);
    assert!(result.success, "Expected program to succeed");
    let output = result.output();
    assert!(
        output.contains("write: 13 bytes"),
        "Expected 'write: 13 bytes' in output"
    );
    assert!(output.contains("read: ok"), "Expected 'read: ok' in output");
}

#[test]
fn test_write_empty_file() {
    let (_temp, path) = temp_test_file("test_empty.txt");
    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()

    match fs.write_file("{}", "")
        Result.Ok(n): println(f"wrote {{n}}")
        Result.Err(e): println("error")

    match fs.read_file("{}")
        Result.Ok(contents): check_empty(contents)
        Result.Err(e): println("read error")

fn check_empty(contents String)
    if contents == ""
        println("empty ok")
    else
        println(f"got {{contents}}")
"#,
        path, path
    );
    assert_runs_with_output(&code, "wrote 0");
    assert_runs_with_output(&code, "empty ok");
}

#[test]
fn test_read_nonexistent_file_returns_error() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()
    match fs.read_file("/tmp/nonexistent_file_xyz.txt")
        Result.Ok(contents): println("unexpected ok")
        Result.Err(error): println("got error")
"#,
        "got error",
    );
}

#[test]
fn test_append_file() {
    let (_temp, path) = temp_test_file("test_append.txt");
    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()

    match fs.write_file("{}", "line1\n")
        Result.Ok(_): println("write ok")
        Result.Err(e): println("write error")

    match fs.append_file("{}", "line2\n")
        Result.Ok(n): println(f"appended {{n}}")
        Result.Err(e): println("append error")

    match fs.read_file("{}")
        Result.Ok(c): check_content(c)
        Result.Err(e): println("read error")

fn check_content(c String)
    if c == "line1\nline2\n"
        println("content ok")
    else
        println("content mismatch")
"#,
        path, path, path
    );
    assert_runs_with_output(&code, "write ok");
    assert_runs_with_output(&code, "appended 6");
    assert_runs_with_output(&code, "content ok");
}

#[test]
fn test_create_dir() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let dir_path = temp_dir.path().join("newdir");
    let path = dir_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()

    match fs.create_dir("{}")
        Result.Ok(was_new): println(f"created {{was_new}}")
        Result.Err(e): println("error")
"#,
        path
    );
    assert_runs_with_output(&code, "created true");
}

#[test]
fn test_delete_file() {
    let (_temp, path) = temp_test_file("test_delete.txt");
    fs::write(&path, "content").expect("write failed");

    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()

    if fs.exists("{}")
        println("exists before")

    match fs.delete("{}")
        Result.Ok(deleted): println(f"deleted {{deleted}}")
        Result.Err(e): println("error")

    if fs.exists("{}")
        println("still exists")
    else
        println("gone")
"#,
        path, path, path
    );
    let result = miri_run(&code);
    assert!(result.success, "Expected program to succeed");
    let output = result.output();
    assert!(
        output.contains("exists before"),
        "Expected 'exists before' in output"
    );
    assert!(
        output.contains("deleted true"),
        "Expected 'deleted true' in output"
    );
    assert!(output.contains("gone"), "Expected 'gone' in output");
}

#[test]
#[ignore = "Result-wrapped list payloads are never freed by the compiler; this causes RC leaks that prevent the test from passing. The issue occurs at the compiler level and is unrelated to the Fs implementation. Re-enable when the compiler's Result<[T], E> deallocation is fixed."]
fn test_list_dir() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    fs::write(temp_dir.path().join("file1.txt"), "").expect("write f1");
    fs::write(temp_dir.path().join("file2.txt"), "").expect("write f2");
    let path = temp_dir.path().to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()

    match fs.list_dir("{}")
        Result.Ok(entries): println(f"count: {{entries.length()}}")
        Result.Err(e): println("error")
"#,
        path
    );
    assert_runs_with_output(&code, "count: 2");
}

#[test]
fn test_cwd() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn check_cwd(dir String)
    if dir.length() > 0
        println("got cwd")
    else
        println("cwd empty")

fn main()
    let fs = Fs()

    match fs.cwd()
        Result.Ok(dir): check_cwd(dir)
        Result.Err(e): println("error")
"#,
        "got cwd",
    );
}
