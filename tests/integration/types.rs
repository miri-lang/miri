// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Type system integration tests.
//!
//! Note: Many type system features (structs, enums, generics) are not yet
//! implemented in the Cranelift backend. These tests use assert_valid for
//! type-checking verification.

use crate::test_utils::{assert_compiles, assert_invalid, assert_valid};

// =============================================================================
// Primitive types (compile tests)
// =============================================================================

#[test]
fn test_integer_types() {
    assert_compiles(
        r#"
fn main() int
    let a = 42
    let b = 100
    a + b
"#,
    );
}

// =============================================================================
// Struct types (type-check only - not yet in codegen)
// =============================================================================

#[test]
fn test_struct_definition_typecheck() {
    assert_valid(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    print(p.x)
    print(p.y)
"#,
    );
}

#[test]
fn test_nested_struct_typecheck() {
    assert_valid(
        r#"
struct Point
    x int
    y int

struct Rectangle
    origin Point
    width int
    height int

fn main()
    let p = Point(x: 0, y: 0)
    let rect = Rectangle(origin: p, width: 10, height: 20)
    print(rect.width)
"#,
    );
}

// =============================================================================
// Enum types (type-check only)
// =============================================================================

#[test]
fn test_enum_definition_typecheck() {
    assert_valid(
        r#"
enum Color
    Red
    Green
    Blue

fn main()
    let c = Color.Red
    match c
        Color.Red: print("Red")
        Color.Green: print("Green")
        Color.Blue: print("Blue")
"#,
    );
}

#[test]
fn test_enum_with_associated_values_typecheck() {
    assert_valid(
        r#"
enum Result
    Ok(int)
    Err(string)

fn main()
    let r = Result.Ok(42)
"#,
    );
}

// =============================================================================
// Generic types (type-check only)
// =============================================================================

#[test]
fn test_generic_struct_typecheck() {
    assert_valid(
        r#"
struct Box<T>
    value T

fn main()
    let b = Box(value: 10)
    print(b.value)
"#,
    );
}

#[test]
fn test_generic_function_typecheck() {
    assert_valid(
        r#"
fn identity<T>(x T) T
    return x

fn main()
    let a = identity(42)
    let b = identity("hello")
"#,
    );
}

// =============================================================================
// Error cases
// =============================================================================

#[test]
fn test_struct_field_access_error() {
    assert_invalid(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    print(p.z)
"#,
        &["Type 'Point' has no field 'z'"],
    );
}

#[test]
fn test_struct_missing_field_error() {
    assert_invalid(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1)
"#,
        &["Missing argument for field 'y'"],
    );
}

#[test]
fn test_type_annotation_mismatch_error() {
    assert_invalid(
        r#"
fn main()
    let x int = "not an int"
"#,
        &["Type mismatch"],
    );
}

#[test]
fn test_undefined_type_error() {
    assert_invalid(
        r#"
fn main()
    let x UndefinedType = 42
"#,
        &["Unknown type", "UndefinedType"],
    );
}
