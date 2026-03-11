// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_for_loop() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

// Order in a Set is not strictly guaranteed by definition,
// but the current implementation likely preserves insertion order
// sequentially. Let's sum them to be safe from ordering issues.
let s = {10, 20, 30}
var sum = 0
for x in s
    sum = sum + x
println(f"{sum}")
"#,
        "60",
    );
}

#[test]
fn test_set_empty_iteration() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s = {1, 2, 3}
s.clear()
var count = 0
for x in s
    count = count + 1
println(f"{count}")
"#,
        "0",
    );
}
