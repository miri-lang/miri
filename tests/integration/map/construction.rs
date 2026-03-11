// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_literal_int_values() {
    assert_runs(r#"let m = {"a": 1, "b": 2}"#);
}

#[test]
fn map_literal_single_entry() {
    assert_runs(r#"let m = {"key": 42}"#);
}

#[test]
fn map_literal_string_values() {
    assert_runs(r#"let m = {"name": "Alice", "city": "NYC"}"#);
}
