// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Miri-tests-Miri: each test reads a `.mi` source file from
//! `tests/stdlib/system/**` and runs it through the real `miri` binary,
//! asserting that the process exits successfully. The `.mi` files themselves
//! perform the actual assertions using `system.testing`.
//!
//! Layout invariant: `tests/stdlib/**` mirrors `src/stdlib/**` exactly. A test
//! for `src/stdlib/system/collections/list.mi` lives at
//! `tests/stdlib/system/collections/list.mi`. The drift check at the bottom
//! enforces this.
//!
//! A failure here means either:
//!   1. the stdlib API drifted from what the `.mi` test exercises, or
//!   2. an assertion fired at runtime — the process aborted with a
//!      `Runtime error: ...` line containing the source location of the
//!      failing assertion. That message is included in the panic output.
//!
//! To run a single suite locally:
//!
//! ```text
//! cargo test --test mod stdlib::mi_suite::test_system_string
//! ```
//!
//! To add a new suite: drop a `<path>.mi` file mirroring the stdlib source
//! path (e.g. `tests/stdlib/system/collections/queryable.mi` for
//! `src/stdlib/system/collections/queryable.mi`) and add a matching
//! `mi_suite_test!(test_system_collections_<name>, "collections/<name>.mi")`
//! invocation below.

use std::path::{Path, PathBuf};

use crate::utils::miri_run;

fn stdlib_test_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("stdlib")
        .join("system")
}

/// Read the `.mi` at `rel_path` (relative to `tests/stdlib/system/`), run it
/// through `miri run`, and require a clean exit. On failure prints stdout+stderr
/// including any `Runtime error: ...` line from a failed in-Miri assertion.
fn run_mi_suite(rel_path: &str) {
    let path = stdlib_test_root().join(rel_path);

    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));

    let result = miri_run(&source);

    if !result.success {
        panic!(
            "Miri-in-Miri suite '{}' failed.\n\n--- output ---\n{}\n--- end ---",
            path.display(),
            result.output()
        );
    }

    if result.stderr.contains("MIRI_LEAK_CHECK: leaked") {
        panic!(
            "Miri-in-Miri suite '{}' leaked memory.\n\n--- output ---\n{}\n--- end ---",
            path.display(),
            result.output()
        );
    }
}

macro_rules! mi_suite_test {
    ($name:ident, $rel:expr) => {
        #[test]
        fn $name() {
            run_mi_suite($rel);
        }
    };
}

mi_suite_test!(test_system_io, "io.mi");
mi_suite_test!(test_system_string, "string.mi");
mi_suite_test!(test_system_math, "math.mi");
mi_suite_test!(test_system_testing, "testing.mi");
mi_suite_test!(test_system_result, "result.mi");
mi_suite_test!(test_system_collections_array, "collections/array.mi");
mi_suite_test!(test_system_collections_list, "collections/list.mi");
mi_suite_test!(test_system_collections_set, "collections/set.mi");
mi_suite_test!(test_system_collections_map, "collections/map.mi");
mi_suite_test!(test_system_collections_tuple, "collections/tuple.mi");
mi_suite_test!(
    test_system_collections_transformable,
    "collections/transformable.mi"
);
mi_suite_test!(test_system_collections_foldable, "collections/foldable.mi");
mi_suite_test!(
    test_system_collections_sequenced,
    "collections/sequenced.mi"
);
mi_suite_test!(test_system_gpu, "gpu.mi");

/// Recursively collect every `.mi` file under `root`, returning paths relative
/// to `base` with forward-slash separators. Used by the drift check below.
fn collect_mi_files_recursive(root: &Path, base: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => panic!("Failed to list {}: {e}", root.display()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_mi_files_recursive(&path, base));
        } else if path.extension().is_some_and(|e| e == "mi") {
            let rel = path
                .strip_prefix(base)
                .expect("entry is under base")
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    out
}

/// Static safety net: every `.mi` file under `tests/stdlib/system/**` must be
/// wired to a `mi_suite_test!` above. If the two drift apart, this test fails
/// and names the offender.
#[test]
fn every_mi_file_is_wired() {
    let wired = [
        "io.mi",
        "string.mi",
        "math.mi",
        "testing.mi",
        "result.mi",
        "collections/array.mi",
        "collections/list.mi",
        "collections/set.mi",
        "collections/map.mi",
        "collections/tuple.mi",
        "collections/transformable.mi",
        "collections/foldable.mi",
        "collections/sequenced.mi",
        "gpu.mi",
    ];

    let root = stdlib_test_root();
    let on_disk = collect_mi_files_recursive(&root, &root);

    let wired_set: std::collections::HashSet<&str> = wired.iter().copied().collect();
    let on_disk_set: std::collections::HashSet<&str> = on_disk.iter().map(String::as_str).collect();

    let missing_runner: Vec<&str> = on_disk_set
        .difference(&wired_set)
        .copied()
        .collect::<Vec<_>>();
    let missing_file: Vec<&str> = wired_set
        .difference(&on_disk_set)
        .copied()
        .collect::<Vec<_>>();

    assert!(
        missing_runner.is_empty() && missing_file.is_empty(),
        "Miri-in-Miri suite drift between tests/stdlib/system/** and mi_suite.rs:\n  \
         .mi files with no test runner: {missing_runner:?}\n  \
         test runners with no .mi file: {missing_file:?}\n\
         Either add the missing `mi_suite_test!(...)` line or delete the orphan."
    );
}
