// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{miri_cmd, BINARY_NAME, VERSION};

#[test]
fn test_version_flag() {
    let mut cmd = miri_cmd();
    let expected = format!("{} {}", BINARY_NAME, VERSION);
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(expected));
}

#[test]
fn test_version_string_content() {
    let version = miri::cli::version_string();
    assert!(version.contains(VERSION));
    assert!(version.contains(std::env::consts::OS));
    assert!(version.contains(std::env::consts::ARCH));
}

#[test]
fn test_version_ref_content() {
    let version_str = miri::cli::version_string();
    let version_ref = miri::cli::version_ref();
    assert_eq!(version_str, version_ref);
}

#[test]
fn test_short_version_flag() {
    let mut cmd = miri_cmd();
    let expected = format!("{} {}", BINARY_NAME, VERSION);
    cmd.arg("-V")
        .assert()
        .success()
        .stdout(predicates::str::contains(expected));
}
