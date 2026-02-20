// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

#[test]
fn test_list_creation() {
    assert_runs("let list = [1, 2, 3, 4, 5]");
}

#[test]
fn test_list_indexing() {
    assert_runs_with_output(
        r#"
use system.io
let list = [10, 20, 30]
print(list[1])
    "#,
        "20",
    );
}

#[test]
fn test_list_index_assignment() {
    assert_runs_with_output(
        r#"
use system.io
var list = [10, 20, 30]
list[1] = 99
print(list[1])
    "#,
        "99",
    );
}
