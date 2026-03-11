// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_forever_with_break() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
forever
    x = x + 1
    if x >= 5
        break
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_forever_with_continue() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
var i = 0
forever
    i = i + 1
    if i > 9
        break
    if i % 2 == 0
        continue
    sum = sum + i
print(f"{sum}")
    "#,
        "25", // 1+3+5+7+9 = 25
    );
}
