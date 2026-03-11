// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn type_alias_with_list() {
    assert_runs(
        r#"
type IntArray is [int; 3]
let nums IntArray = [1, 2, 3]
"#,
    );
}

#[test]
fn type_alias_with_map() {
    assert_runs(
        r#"
type StringIntMap is {String: int}
let map StringIntMap = {"a": 1, "b": 2}
"#,
    );
}

#[test]
fn type_alias_with_tuple() {
    assert_runs(
        r#"
type Pair is (int, int)
let p Pair = (1, 2)
"#,
    );
}

#[test]
fn type_alias_deeply_nested() {
    assert_runs(
        r#"
type IntArray is [int; 2]
type IntArrayArray is [[int; 2]; 2]
let deep IntArrayArray = [[1, 2], [3, 4]]
"#,
    );
}
