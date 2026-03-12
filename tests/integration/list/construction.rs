// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_construction_int() {
    assert_runs(
        "
use system.collections.list

let l = List([1, 2, 3])
",
    );
}

#[test]
fn list_construction_string() {
    assert_runs(
        "
use system.collections.list

let l = List([\"hello\", \"world\"])
",
    );
}
