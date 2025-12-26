// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::test_utils::{assert_invalid, assert_valid};

#[test]
fn test_if_else() {
    assert_valid(
        r#"
fn main()
    let a = 10
    if a > 5
        print("a is greater than 5")
    else
        print("a is less than or equal to 5")
"#,
    );
}

#[test]
fn test_while_loop() {
    assert_valid(
        r#"
fn main()
    var i = 0
    while i < 10
        print(i)
        i = i + 1
"#,
    );
}

#[test]
fn test_for_loop() {
    assert_valid(
        r#"
fn main()
    for i in [1, 2, 3]
        print(i)
"#,
    );
}

#[test]
fn test_break_continue() {
    assert_valid(
        r#"
fn main()
    var i = 0
    while i < 10
        i = i + 1
        if i == 5
            continue
        if i == 8
            break
        print(i)
"#,
    );
}

#[test]
fn test_invalid_break_outside_loop() {
    assert_invalid(
        r#"
fn main()
    break
"#,
        &["Break statement outside of loop"],
    );
}
