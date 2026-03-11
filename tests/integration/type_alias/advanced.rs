// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn type_alias_with_nullable() {
    assert_runs(
        r#"
type OptionalInt is int?
var x OptionalInt = 5
x = None
"#,
    );
}

#[test]
fn type_alias_in_struct() {
    assert_runs(
        r#"
type MyInt is int

struct Point
    x MyInt
    y MyInt

let p = Point(1, 2)
"#,
    );
}

#[test]
fn type_alias_in_for_loop() {
    assert_runs(
        r#"
type Numbers is [int; 3]
let nums Numbers = [1, 2, 3]
for n in nums
    let x = n * 2
"#,
    );
}
