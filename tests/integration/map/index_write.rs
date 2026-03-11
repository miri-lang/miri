// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_index_write() {
    assert_runs_with_output(
        r#"
use system.io

var m = {"a": 1}
m["a"] = 10
let v = m["a"]
println(f"{v}")
"#,
        "10",
    );
}

#[test]
fn map_index_write_new_key() {
    assert_runs_with_output(
        r#"
use system.io

var m = {"a": 1}
m["b"] = 2
let v = m["b"]
println(f"{v}")
"#,
        "2",
    );
}
