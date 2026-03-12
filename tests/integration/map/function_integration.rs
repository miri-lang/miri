// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_passed_to_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn get_value(m Map<String, int>, key String) int
    return m[key]

fn main()
    let m = {"x": 42}
    let v = get_value(m: m, key: "x")
    println(f"{v}")
"#,
        "42",
    );
}

#[test]
fn map_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn make_map() Map<String, int>
    return {"a": 100}

fn main()
    let m = make_map()
    println(f"{m['a']}")
"#,
        "100",
    );
}
