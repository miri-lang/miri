// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::test_utils::{assert_invalid, assert_valid};

#[test]
fn test_hello_world() {
    assert_valid(
        r#"
fn main()
    print("Hello, World!")
"#,
    );
}

#[test]
fn test_variables() {
    assert_valid(
        r#"
fn main()
    let a = 10
    var b = 20
    b = 30
    let c = a + b
    print(c)
"#,
    );
}

#[test]
fn test_variable_shadowing() {
    assert_valid(
        r#"
fn main()
    let a = 10
    if true
        let a = 20
        print(a)
    print(a)
"#,
    );
}

#[test]
fn test_invalid_variable_reassignment() {
    assert_invalid(
        r#"
fn main()
    let a = 10
    a = 20
"#,
        &["Cannot assign to immutable variable 'a'"],
    );
}

#[test]
fn test_invalid_type_mismatch() {
    assert_invalid(
        r#"
fn main()
    let a int = "string"
"#,
        &["Type mismatch"],
    );
}
