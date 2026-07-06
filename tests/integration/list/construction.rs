// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_construction_int() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([1, 2, 3])
println(f\"{l.length()}\")
",
        "3",
    );
}

#[test]
fn list_construction_string() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([\"hello\", \"world\"])
println(f\"{l.length()}\")
",
        "2",
    );
}
