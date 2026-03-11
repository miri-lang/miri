// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_index_read_int_value() {
    assert_runs_with_output(
        r#"
use system.io

let m = {"a": 1, "b": 2, "c": 3}
let a = m["a"]
let b = m["b"]
let c = m["c"]
println(f"{a}")
println(f"{b}")
println(f"{c}")
"#,
        "1\n2\n3",
    );
}

#[test]
fn map_index_read_single_entry() {
    assert_runs_with_output(
        r#"
use system.io

let m = {"key": 42}
let v = m["key"]
println(f"{v}")
"#,
        "42",
    );
}
