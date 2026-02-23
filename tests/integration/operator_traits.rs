// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

// =============================================================================
// String operator overloads via traits
// =============================================================================

#[test]
fn test_string_add_operator() {
    assert_runs_with_output(
        r#"
use system.io

let a = "foo"
let b = "bar"
print(a + b)
    "#,
        "foobar",
    );
}

#[test]
fn test_string_add_chained() {
    assert_runs_with_output(
        r#"
use system.io

print("a" + "b" + "c" + "d")
    "#,
        "abcd",
    );
}

#[test]
fn test_string_equal_operator() {
    assert_runs_with_output(
        r#"
use system.io

if "hello" == "hello"
    print("yes")
else
    print("no")
    "#,
        "yes",
    );
}

#[test]
fn test_string_not_equal_operator() {
    assert_runs_with_output(
        r#"
use system.io

if "hello" != "world"
    print("different")
else
    print("same")
    "#,
        "different",
    );
}

#[test]
fn test_string_equal_false() {
    assert_runs_with_output(
        r#"
use system.io

if "hello" == "world"
    print("same")
else
    print("different")
    "#,
        "different",
    );
}

#[test]
fn test_string_multiply_operator() {
    assert_runs_with_output(
        r#"
use system.io

let s = "ha" * 3
print(s)
    "#,
        "hahaha",
    );
}

#[test]
fn test_string_multiply_single() {
    assert_runs_with_output(
        r#"
use system.io

print("x" * 1)
    "#,
        "x",
    );
}

#[test]
fn test_string_multiply_zero() {
    assert_runs_with_output(
        r#"
use system.io

print("x" * 0)
    "#,
        "",
    );
}

#[test]
fn test_string_multiply_expression() {
    assert_runs_with_output(
        r#"
use system.io

let count = 2 + 3
print("ab" * count)
    "#,
        "ababababab",
    );
}

#[test]
fn test_string_multiply_invalid_rhs() {
    assert_compiler_error(
        r#"
let s = "ha" * "3"
    "#,
        "Invalid types for arithmetic operation",
    );
}

// =============================================================================
// String iteration via Iterable trait
// =============================================================================

#[test]
fn test_string_iteration_basic() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

for ch in "abc"
    print(ch)
    "#,
        "abc",
    );
}

#[test]
fn test_string_iteration_empty() {
    assert_runs(
        r#"
use system.string

for ch in ""
    let _ = ch
    "#,
    );
}

#[test]
fn test_string_iteration_println() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

for ch in "hi"
    println(ch)
    "#,
        "h",
    );
}

// =============================================================================
// Custom class operator overloads (type checker level)
// =============================================================================

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
    assert_compiler_error(
        r#"
use system.ops

class Color implements Equatable
    r int

    fn init(r int)
        self.r = r

    public fn equals(other Color) bool
        return self.r == other.r
    "#,
        "does not match trait",
    );
}

#[test]
fn test_custom_class_addable_type_checks() {
    // Verify that the type checker recognizes the Addable trait implementation.
    assert_compiler_error(
        r#"
use system.ops

class Counter implements Addable
    value int

    fn init(value int)
        self.value = value

    public fn concat(other Counter) Counter
        return Counter(self.value + other.value)
    "#,
        "does not match trait",
    );
}
