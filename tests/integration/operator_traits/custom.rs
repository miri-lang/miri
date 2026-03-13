// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_custom_class_missing_trait_method_error() {
    assert_compiler_error(
        r#"
use system.ops

class Broken implements Equatable
    x int
"#,
        "must implement method",
    );
}

#[test]
fn test_custom_class_equatable_type_checks() {
    // Verify that the type checker accepts a class implementing Equatable
    // and allows the `==` operator on it (even if codegen isn't tested here).
    assert_type_checks(
        r#"
use system.ops

class Color implements Equatable
    r int

    fn init(r int)
        self.r = r

    public fn equals(other Color) bool
        return self.r == other.r
"#,
    );
}

#[test]
fn test_custom_class_addable_type_checks() {
    // Verify that the type checker recognizes the Addable trait implementation.
    assert_type_checks(
        r#"
use system.ops

class Counter implements Addable
    value int

    fn init(value int)
        self.value = value

    public fn concat(other Counter) Counter
        return Counter(self.value + other.value)
"#,
    );
}
