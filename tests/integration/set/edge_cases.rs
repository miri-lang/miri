// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_clear_empty() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s = {1}
s.clear()
// Second clear should be safe
s.clear()
println(f"{s.is_empty()}")
"#,
        "true",
    );
}

#[test]
fn test_set_element_at_out_of_bounds() {
    // Runtime's miri_rt_set_element_at returns 0 on OOB index (no crash).
    assert_runs(
        r#"
use system.collections.set
let s = {1, 2, 3}
let x = s.element_at(99)
"#,
    );
}

#[test]
fn test_set_large_reallocation() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

var s = {0}
var i = 1
while i < 100
    s.add(i)
    i = i + 1

println(f"{s.length()}")
println(f"{s.contains(50)}")
println(f"{s.contains(99)}")
"#,
        "100\ntrue\ntrue",
    );
}
