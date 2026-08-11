// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use tempfile::TempDir;

#[test]
fn test_read_missing_file_returns_notfound() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn handle_read_error(e FsError)
    match e
        FsError.NotFound(path): println(f"NotFound: {path}")
        default: println("other error")

fn main()
    let fs = Fs()
    match fs.read_file("/tmp/nonexistent_xyz_abc_123.txt")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_read_error(e)
"#,
        "NotFound: /tmp/nonexistent_xyz_abc_123.txt",
    );
}

#[test]
fn test_read_directory_returns_notadirectory() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_read_error(e FsError)
    match e
        FsError.NotADirectory(p): println("NotADirectory")
        default: println("other error")

fn main()
    let fs = Fs()
    match fs.read_file("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_read_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "NotADirectory");
}

#[test]
fn test_delete_missing_file_returns_notfound() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn handle_delete_error(e FsError)
    match e
        FsError.NotFound(_): println("NotFound")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.delete("/tmp/nonexistent_delete_xyz.txt")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_delete_error(e)
"#,
        "NotFound",
    );
}

#[test]
fn test_delete_directory_returns_notadirectory() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_delete_error(e FsError)
    match e
        FsError.NotADirectory(_): println("NotADirectory")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.delete("{}")
        Result.Ok(_): println("unexpected")
        Result.Err(e): handle_delete_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "NotADirectory");
}

#[test]
fn test_list_dir_missing_returns_notfound() {
    assert_runs_with_output(
        r#"
use system.result
use system.fs

fn handle_list_error(e FsError)
    match e
        FsError.NotFound(_): println("NotFound")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.list_dir("/tmp/nonexistent_list_xyz.txt")
        Result.Ok(_): println("unexpected")
        Result.Err(e): handle_list_error(e)
"#,
        "NotFound",
    );
}

#[test]
fn test_list_dir_on_file_returns_notadirectory() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("test_file.txt");
    std::fs::write(&file_path, "test").expect("failed to write test file");
    let path = file_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_list_error(e FsError)
    match e
        FsError.NotADirectory(_): println("NotADirectory")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.list_dir("{}")
        Result.Ok(_): println("unexpected")
        Result.Err(e): handle_list_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "NotADirectory");
}

#[test]
fn test_create_dir_idempotent() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let dir_path = temp_dir.path().join("testdir");
    let path = dir_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn main()
    let fs = Fs()

    match fs.create_dir("{}")
        Result.Ok(was_new): println(f"first: {{was_new}}")
        Result.Err(e): println("error1")

    match fs.create_dir("{}")
        Result.Ok(was_new): println(f"second: {{was_new}}")
        Result.Err(e): println("error2")
"#,
        path, path
    );
    assert_runs_with_output(&code, "first: true");
    assert_runs_with_output(&code, "second: false");
}

#[test]
fn test_create_dir_on_existing_file_returns_already_exists() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("existing_file.txt");
    std::fs::write(&file_path, "test").expect("failed to write test file");
    let path = file_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_create_error(e FsError)
    match e
        FsError.AlreadyExists(_): println("AlreadyExists")
        default: println("other")

fn main()
    let fs = Fs()

    match fs.create_dir("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_create_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "AlreadyExists");
}

#[test]
fn test_write_permission_denied() {
    use std::fs::File;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("readonly.txt");
    File::create(&file_path).expect("failed to create test file");

    // Get current user ID
    #[cfg(unix)]
    {
        use std::process::Command;
        let id_output = Command::new("id")
            .arg("-u")
            .output()
            .expect("failed to run id command");
        let uid_str = String::from_utf8_lossy(&id_output.stdout);
        if uid_str.trim() == "0" {
            eprintln!("Running as root - permission tests don't work, skipping");
            return;
        }
    }

    // Remove write permissions
    let perms = std::fs::Permissions::from_mode(0o444);
    std::fs::set_permissions(&file_path, perms).expect("failed to set permissions");

    let path = file_path.to_string_lossy().to_string();
    let code = format!(
        r#"
use system.result
use system.fs

fn handle_write_error(e FsError)
    match e
        FsError.PermissionDenied(_): println("PermissionDenied")
        default: println("other")

fn main()
    let fs = Fs()

    match fs.write_file("{}", "new content")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_write_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "PermissionDenied");

    // Restore permissions so cleanup can delete the file
    let perms = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(&file_path, perms).expect("failed to restore permissions");
}

#[test]
fn test_read_file_invalid_data() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("non_utf8.bin");
    // Write non-UTF-8 bytes directly
    std::fs::write(&file_path, &[0xFF, 0xFE]).expect("failed to write test file");
    let path = file_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_read_error(e FsError)
    match e
        FsError.InvalidData(_): println("InvalidData")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.read_file("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_read_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "InvalidData");
}

#[test]
fn test_read_file_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("noperm.txt");
    std::fs::write(&file_path, "test").expect("failed to write test file");

    // Skip if running as root
    #[cfg(unix)]
    {
        use std::process::Command;
        let id_output = Command::new("id")
            .arg("-u")
            .output()
            .expect("failed to run id command");
        let uid_str = String::from_utf8_lossy(&id_output.stdout);
        if uid_str.trim() == "0" {
            eprintln!("Running as root - permission tests don't work, skipping");
            return;
        }
    }

    // Remove read permissions
    let perms = std::fs::Permissions::from_mode(0o000);
    std::fs::set_permissions(&file_path, perms).expect("failed to set permissions");

    let path = file_path.to_string_lossy().to_string();
    let code = format!(
        r#"
use system.result
use system.fs

fn handle_read_error(e FsError)
    match e
        FsError.PermissionDenied(_): println("PermissionDenied")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.read_file("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_read_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "PermissionDenied");

    // Restore permissions so cleanup can delete the file
    let perms = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(&file_path, perms).expect("failed to restore permissions");
}

#[test]
fn test_write_file_not_found_parent() {
    let code = r#"
use system.result
use system.fs

fn handle_write_error(e FsError)
    match e
        FsError.NotFound(_): println("NotFound")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.write_file("/nonexistent/parent/dir/file.txt", "content")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_write_error(e)
"#;
    assert_runs_with_output(code, "NotFound");
}

#[test]
fn test_write_file_not_a_directory() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let dir_path = temp_dir.path().join("dir");
    std::fs::create_dir(&dir_path).expect("failed to create directory");
    let path = dir_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_write_error(e FsError)
    match e
        FsError.NotADirectory(_): println("NotADirectory")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.write_file("{}", "new content")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_write_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "NotADirectory");
}

#[test]
fn test_append_file_not_found_parent() {
    let code = r#"
use system.result
use system.fs

fn handle_append_error(e FsError)
    match e
        FsError.NotFound(_): println("NotFound")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.append_file("/nonexistent/parent/dir/file.txt", "content")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_append_error(e)
"#;
    assert_runs_with_output(code, "NotFound");
}

#[test]
fn test_append_file_not_a_directory() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let dir_path = temp_dir.path().join("dir");
    std::fs::create_dir(&dir_path).expect("failed to create directory");
    let path = dir_path.to_string_lossy().to_string();

    let code = format!(
        r#"
use system.result
use system.fs

fn handle_append_error(e FsError)
    match e
        FsError.NotADirectory(_): println("NotADirectory")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.append_file("{}", "new content")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_append_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "NotADirectory");
}

#[test]
fn test_append_file_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("noperm_append.txt");
    std::fs::write(&file_path, "initial").expect("failed to write test file");

    // Skip if running as root
    #[cfg(unix)]
    {
        use std::process::Command;
        let id_output = Command::new("id")
            .arg("-u")
            .output()
            .expect("failed to run id command");
        let uid_str = String::from_utf8_lossy(&id_output.stdout);
        if uid_str.trim() == "0" {
            eprintln!("Running as root - permission tests don't work, skipping");
            return;
        }
    }

    // Remove write permissions
    let perms = std::fs::Permissions::from_mode(0o444);
    std::fs::set_permissions(&file_path, perms).expect("failed to set permissions");

    let path = file_path.to_string_lossy().to_string();
    let code = format!(
        r#"
use system.result
use system.fs

fn handle_append_error(e FsError)
    match e
        FsError.PermissionDenied(_): println("PermissionDenied")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.append_file("{}", "new")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_append_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "PermissionDenied");

    // Restore permissions so cleanup can delete the file
    let perms = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(&file_path, perms).expect("failed to restore permissions");
}

#[test]
fn test_create_dir_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let readonly_dir = temp_dir.path().join("readonly");
    std::fs::create_dir(&readonly_dir).expect("failed to create readonly dir");

    // Skip if running as root
    #[cfg(unix)]
    {
        use std::process::Command;
        let id_output = Command::new("id")
            .arg("-u")
            .output()
            .expect("failed to run id command");
        let uid_str = String::from_utf8_lossy(&id_output.stdout);
        if uid_str.trim() == "0" {
            eprintln!("Running as root - permission tests don't work, skipping");
            return;
        }
    }

    // Remove write permissions from parent
    let perms = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(&readonly_dir, perms).expect("failed to set permissions");

    let child_path = readonly_dir.join("subdir").to_string_lossy().to_string();
    let code = format!(
        r#"
use system.result
use system.fs

fn handle_create_error(e FsError)
    match e
        FsError.PermissionDenied(_): println("PermissionDenied")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.create_dir("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_create_error(e)
"#,
        child_path
    );
    assert_runs_with_output(&code, "PermissionDenied");

    // Restore permissions so cleanup can delete the directory
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&readonly_dir, perms).expect("failed to restore permissions");
}

#[test]
fn test_delete_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let file_path = temp_dir.path().join("to_delete.txt");
    std::fs::write(&file_path, "content").expect("failed to write test file");

    // Skip if running as root
    #[cfg(unix)]
    {
        use std::process::Command;
        let id_output = Command::new("id")
            .arg("-u")
            .output()
            .expect("failed to run id command");
        let uid_str = String::from_utf8_lossy(&id_output.stdout);
        if uid_str.trim() == "0" {
            eprintln!("Running as root - permission tests don't work, skipping");
            return;
        }
    }

    // Remove write permissions from parent directory
    let parent_dir = file_path.parent().unwrap();
    let perms = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(parent_dir, perms).expect("failed to set permissions");

    let path = file_path.to_string_lossy().to_string();
    let code = format!(
        r#"
use system.result
use system.fs

fn handle_delete_error(e FsError)
    match e
        FsError.PermissionDenied(_): println("PermissionDenied")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.delete("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_delete_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "PermissionDenied");

    // Restore permissions so cleanup can delete the file
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(parent_dir, perms).expect("failed to restore permissions");
}

#[test]
fn test_list_dir_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let list_dir = temp_dir.path().join("noperm_dir");
    std::fs::create_dir(&list_dir).expect("failed to create test dir");

    // Skip if running as root
    #[cfg(unix)]
    {
        use std::process::Command;
        let id_output = Command::new("id")
            .arg("-u")
            .output()
            .expect("failed to run id command");
        let uid_str = String::from_utf8_lossy(&id_output.stdout);
        if uid_str.trim() == "0" {
            eprintln!("Running as root - permission tests don't work, skipping");
            return;
        }
    }

    // Remove read permissions
    let perms = std::fs::Permissions::from_mode(0o000);
    std::fs::set_permissions(&list_dir, perms).expect("failed to set permissions");

    let path = list_dir.to_string_lossy().to_string();
    let code = format!(
        r#"
use system.result
use system.fs

fn handle_list_error(e FsError)
    match e
        FsError.PermissionDenied(_): println("PermissionDenied")
        default: println("other")

fn main()
    let fs = Fs()
    match fs.list_dir("{}")
        Result.Ok(_): println("unexpected ok")
        Result.Err(e): handle_list_error(e)
"#,
        path
    );
    assert_runs_with_output(&code, "PermissionDenied");

    // Restore permissions so cleanup can delete the directory
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&list_dir, perms).expect("failed to restore permissions");
}

#[test]
fn test_private_ffi_not_accessible() {
    assert_compiler_error(
        r#"
fn main()
    miri_rt_fs_status()
"#,
        "Undefined variable: miri_rt_fs_status",
    );
}
