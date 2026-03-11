// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_map_alias_no_double_free() {
    assert_runs(
        r#"
use system.collections.map
let m1 = {"a": 1, "b": 2}
let m2 = m1 // IncRef
// Both out of scope, shouldn't crash
"#,
    );
}

#[test]
fn test_map_reassign_frees_old() {
    assert_runs(
        r#"
use system.collections.map
var m = {"a": 1}
m = {"b": 2} // frees old
"#,
    );
}

#[test]
fn test_map_passed_to_function_no_dangle() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn consume(m Map<String, int>)
    // goes out of scope

fn main()
    let m = {"k": 99}
    consume(m)
    println(f"{m.length()}")
"#,
        "1",
    );
}
