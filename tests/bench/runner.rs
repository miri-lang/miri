// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The benchmark runner's own tests.
//!
//! `bench/run.py` is Python, standard library only, so its unit tests are
//! Python too (`tests/bench/python/`); this runs them as part of the suite. It
//! skips with a line saying so when `python3` is not installed.

use std::path::PathBuf;
use std::process::Command;

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn test_bench_runner_unit_tests_pass() {
    if !python3_available() {
        println!("skipping: python3 is not on PATH, so the runner's tests were not run");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .args([
            "-W",
            "error::ResourceWarning",
            "-m",
            "unittest",
            "discover",
            "-s",
        ])
        .arg(root.join("tests").join("bench").join("python"))
        .current_dir(&root)
        .output()
        .expect("python3 runs");
    assert!(
        output.status.success(),
        "the runner's unit tests failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
