// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_for_loop() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (1, 2, 3)
for x in t
    println(f"{x}")
"#,
        "1\n2\n3",
    );
}

#[test]
fn test_tuple_for_loop_sum() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
var sum = 0
for x in t
    sum = sum + x
println(f"{sum}")
"#,
        "60",
    );
}
