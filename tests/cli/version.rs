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
fn test_short_version_flag() {
    let mut cmd = miri_cmd();
    let expected = format!("{} {}", BINARY_NAME, VERSION);
    cmd.arg("-V")
        .assert()
        .success()
        .stdout(predicates::str::contains(expected));
}
