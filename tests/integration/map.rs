// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs;

#[test]
fn test_map_creation() {
    assert_runs(r#"let m = {"a": 1, "b": 2}"#);
}

#[test]
fn test_map_single_entry() {
    assert_runs(r#"let m = {"key": 42}"#);
}
