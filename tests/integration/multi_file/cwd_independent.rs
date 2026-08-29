// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{
    miri_check_project_with_cwd, miri_run_project_with_cwd, miri_test_project_with_cwd, strip_ansi,
    WorkingDirMode,
};

/// A bare `use util` sibling import must work from the project root.
#[test]
fn test_bare_use_sibling_from_project_root() {
    assert_runs_from_cwd(
        &[
            ("main.mi", "use util\nprintln(f\"{helper()}\")\n"),
            ("util.mi", "fn helper() int:\n    return 42\n"),
        ],
        WorkingDirMode::ProjectRoot,
        None,
        "42",
    );
}

/// A bare `use util` sibling import must work from the project's parent directory.
#[test]
fn test_bare_use_sibling_from_project_parent() {
    assert_runs_from_cwd(
        &[
            ("main.mi", "use util\nprintln(f\"{helper()}\")\n"),
            ("util.mi", "fn helper() int:\n    return 42\n"),
        ],
        WorkingDirMode::ProjectParent,
        None,
        "42",
    );
}

/// A bare `use util` sibling import must work from the filesystem root.
#[test]
fn test_bare_use_sibling_from_filesystem_root() {
    assert_runs_from_cwd(
        &[
            ("main.mi", "use util\nprintln(f\"{helper()}\")\n"),
            ("util.mi", "fn helper() int:\n    return 42\n"),
        ],
        WorkingDirMode::FilesystemRoot,
        None,
        "42",
    );
}

/// A `use local.util` import must work from the project root.
#[test]
fn test_local_use_from_project_root() {
    assert_runs_from_cwd(
        &[
            ("main.mi", "use local.util\nprintln(f\"{helper()}\")\n"),
            ("util.mi", "fn helper() int:\n    return 99\n"),
        ],
        WorkingDirMode::ProjectRoot,
        None,
        "99",
    );
}

/// A `use local.util` import must work from the project's parent directory.
#[test]
fn test_local_use_from_project_parent() {
    assert_runs_from_cwd(
        &[
            ("main.mi", "use local.util\nprintln(f\"{helper()}\")\n"),
            ("util.mi", "fn helper() int:\n    return 99\n"),
        ],
        WorkingDirMode::ProjectParent,
        None,
        "99",
    );
}

/// A `use local.util` import must work from the filesystem root.
#[test]
fn test_local_use_from_filesystem_root() {
    assert_runs_from_cwd(
        &[
            ("main.mi", "use local.util\nprintln(f\"{helper()}\")\n"),
            ("util.mi", "fn helper() int:\n    return 99\n"),
        ],
        WorkingDirMode::FilesystemRoot,
        None,
        "99",
    );
}

/// Error path: an unresolvable module reports MER_NAM_002 with help text.
#[test]
fn test_unresolvable_module_help_lists_search_roots() {
    let result = miri_run_project_with_cwd(
        &[("main.mi", "use missing_module\nprintln(\"test\")\n")],
        WorkingDirMode::ProjectRoot,
        None,
    );

    if result.success {
        panic!("Expected compilation to fail for missing module");
    }

    let output = strip_ansi(&result.output());

    // Check for the error code
    if !output.contains("MER_NAM_002") {
        panic!("Expected MER_NAM_002 in error output, but got:\n{}", output);
    }

    // Check for the module not found message
    if !output.contains("Module 'missing_module' not found") {
        panic!(
            "Expected 'Module 'missing_module' not found' in error output, but got:\n{}",
            output
        );
    }

    // Check for help text listing search roots
    if !output.contains("Module was not found in any of these search roots") {
        panic!(
            "Expected help text about search roots, but got:\n{}",
            output
        );
    }

    // Check that MIRI_STDLIB_PATH is mentioned
    if !output.contains("MIRI_STDLIB_PATH") {
        panic!(
            "Expected MIRI_STDLIB_PATH to be mentioned in help text, but got:\n{}",
            output
        );
    }
}

/// Helper to run a project from a specific working directory, optionally with a custom
/// stdlib path, asserting it prints the expected output.
fn assert_runs_from_cwd(
    files: &[(&str, &str)],
    working_dir_mode: WorkingDirMode,
    stdlib_path_override: Option<&std::path::Path>,
    expected_output: &str,
) {
    let result = miri_run_project_with_cwd(files, working_dir_mode, stdlib_path_override);

    if !result.success {
        if result.stderr.contains("MIRI_LEAK_CHECK: leaked") {
            panic!("Memory leak detected:\n{}", result.output());
        }
        if result.stderr.contains("MIRI_HEAP_GUARD:") {
            panic!("Heap guard reported a violation:\n{}", result.output());
        }
        panic!(
            "Expected project to compile and run successfully, but it failed:\n{}",
            result.output()
        );
    }

    if !result.output().contains(expected_output) {
        panic!(
            "Expected output '{}' not found in project output:\n{}",
            expected_output,
            result.output()
        );
    }
}

// Tests for `miri check` command

/// A bare `use util` sibling import must work with `miri check` from the project root.
#[test]
fn test_check_bare_use_sibling_from_project_root() {
    let result = miri_check_project_with_cwd(
        &[
            ("main.mi", "use util\nfn helper() int:\n    return 42\n"),
            ("util.mi", "fn util_func() int:\n    return 99\n"),
        ],
        WorkingDirMode::ProjectRoot,
        None,
    );

    if !result.success {
        panic!(
            "Expected check to succeed from project root, but it failed:\n{}",
            result.output()
        );
    }
}

/// A bare `use util` sibling import must work with `miri check` from the project's parent directory.
#[test]
fn test_check_bare_use_sibling_from_project_parent() {
    let result = miri_check_project_with_cwd(
        &[
            ("main.mi", "use util\nfn helper() int:\n    return 42\n"),
            ("util.mi", "fn util_func() int:\n    return 99\n"),
        ],
        WorkingDirMode::ProjectParent,
        None,
    );

    if !result.success {
        panic!(
            "Expected check to succeed from project parent, but it failed:\n{}",
            result.output()
        );
    }
}

/// A bare `use util` sibling import must work with `miri check` from the filesystem root.
#[test]
fn test_check_bare_use_sibling_from_filesystem_root() {
    let result = miri_check_project_with_cwd(
        &[
            ("main.mi", "use util\nfn helper() int:\n    return 42\n"),
            ("util.mi", "fn util_func() int:\n    return 99\n"),
        ],
        WorkingDirMode::FilesystemRoot,
        None,
    );

    if !result.success {
        panic!(
            "Expected check to succeed from filesystem root, but it failed:\n{}",
            result.output()
        );
    }
}

// Tests for `miri test` command

/// A `miri test` command must work from the project root without MIRI_STDLIB_PATH.
/// The test imports a sibling module and tests a function from it.
#[test]
fn test_test_from_project_root() {
    let result = miri_test_project_with_cwd(
        &[
            (
                "main.mi",
                "use system.testing\nuse util\n\n@test\nfn test_helper()\n    assert_eq(helper(), 42)\n",
            ),
            ("util.mi", "public fn helper() int:\n    return 42\n"),
        ],
        WorkingDirMode::ProjectRoot,
        None,
    );

    if !result.success {
        panic!(
            "Expected test to succeed from project root, but it failed:\n{}",
            result.output()
        );
    }
}

/// A `miri test` command must work from the project's parent directory without MIRI_STDLIB_PATH.
/// The test imports a sibling module and tests a function from it.
#[test]
fn test_test_from_project_parent() {
    let result = miri_test_project_with_cwd(
        &[
            (
                "main.mi",
                "use system.testing\nuse util\n\n@test\nfn test_helper()\n    assert_eq(helper(), 42)\n",
            ),
            ("util.mi", "public fn helper() int:\n    return 42\n"),
        ],
        WorkingDirMode::ProjectParent,
        None,
    );

    if !result.success {
        panic!(
            "Expected test to succeed from project parent, but it failed:\n{}",
            result.output()
        );
    }
}

/// A `miri test` command must work from the filesystem root without MIRI_STDLIB_PATH.
/// The test imports a sibling module and tests a function from it.
#[test]
fn test_test_from_filesystem_root() {
    let result = miri_test_project_with_cwd(
        &[
            (
                "main.mi",
                "use system.testing\nuse util\n\n@test\nfn test_helper()\n    assert_eq(helper(), 42)\n",
            ),
            ("util.mi", "public fn helper() int:\n    return 42\n"),
        ],
        WorkingDirMode::FilesystemRoot,
        None,
    );

    if !result.success {
        panic!(
            "Expected test to succeed from filesystem root, but it failed:\n{}",
            result.output()
        );
    }
}

// Tests for stdlib shadowing and precedence

/// Standard library cannot be shadowed by a user-defined module.
/// Even if a user creates `system/io.mi`, the real stdlib `system/io` wins.
/// This test imports `system.io` and calls `line_end()`, which is defined only
/// in the real stdlib. A decoy `system/io.mi` without `line_end()` would fail
/// to compile if it were picked instead of the real stdlib.
#[test]
fn test_stdlib_cannot_be_shadowed() {
    let result = miri_run_project_with_cwd(
        &[
            (
                "main.mi",
                "use system.io\nprintln(f\"line_end={line_end()}\")\n",
            ),
            (
                "system/io.mi",
                "fn print(s String) unit:\n    println(\"decoy_stdlib\")\n",
            ),
        ],
        WorkingDirMode::ProjectRoot,
        None,
    );

    if !result.success {
        panic!(
            "Expected program using real stdlib to succeed, but it failed:\n{}",
            result.output()
        );
    }

    // The output should contain the real stdlib's line_end() output
    let output = result.output();
    if !output.contains("line_end=") {
        panic!(
            "Expected output 'line_end=' from real stdlib's line_end() function, but got:\n{}",
            output
        );
    }
}

/// MIRI_STDLIB_PATH should take priority over the manifest fallback.
/// This test verifies that a custom stdlib root is honored when MIRI_STDLIB_PATH is set.
/// The custom stdlib provides a local module that differs from the project, proving
/// the custom stdlib was consulted when the override is active.
#[test]
fn test_miri_stdlib_path_takes_priority() {
    use std::fs;
    use tempfile::tempdir;

    // Create a custom stdlib directory with a custom helper module
    let stdlib_dir = tempdir().unwrap();

    // Create a custom helper.mi module that will only be found if stdlib_path is honored
    fs::write(
        stdlib_dir.path().join("helper.mi"),
        "public fn get_message() String:\n    return \"from_custom_stdlib\"\n",
    )
    .unwrap();

    // Create the project that imports the custom module
    let result = miri_run_project_with_cwd(
        &[("main.mi", "use helper\nprintln(get_message())\n")],
        WorkingDirMode::ProjectRoot,
        Some(stdlib_dir.path()),
    );

    if !result.success {
        panic!(
            "Expected run with MIRI_STDLIB_PATH to succeed (using custom stdlib):\n{}",
            result.output()
        );
    }

    // Verify the custom function was called (proving custom stdlib was used)
    if !result.output().contains("from_custom_stdlib") {
        panic!(
            "Expected output 'from_custom_stdlib' from custom stdlib, but got:\n{}",
            result.output()
        );
    }
}
