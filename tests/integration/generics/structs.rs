// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// --- Acceptance criteria tests ---

#[test]
fn test_generic_struct_int_construction_and_field_access() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<T>
    value T

fn main()
    let w = Wrapper<int>(value: 42)
    println(f"{w.value}")
    "#,
        "42",
    );
}

#[test]
fn test_generic_struct_string_construction_and_field_access() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<T>
    value T

fn main()
    let s = Wrapper<String>(value: "hi")
    println(s.value)
    "#,
        "hi",
    );
}

#[test]
fn test_generic_struct_two_instantiations() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<T>
    value T

fn main()
    let w = Wrapper<int>(value: 42)
    let s = Wrapper<String>(value: "hi")
    println(f"{w.value}")
    println(s.value)
    "#,
        "42",
    );
}

// --- Additional functionality tests ---

#[test]
fn test_generic_struct_bool_field() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<T>
    value T

fn main()
    let b = Wrapper<bool>(value: true)
    println(f"{b.value}")
    "#,
        "true",
    );
}

#[test]
fn test_generic_struct_two_fields() {
    assert_runs_with_output(
        r#"
use system.io

struct Pair<T>
    first T
    second T

fn main()
    let p = Pair<int>(first: 10, second: 20)
    println(f"{p.first}")
    println(f"{p.second}")
    "#,
        "10",
    );
}

#[test]
fn test_generic_struct_field_in_expression() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<T>
    value T

fn main()
    let w = Wrapper<int>(value: 5)
    let doubled = w.value * 2
    println(f"{doubled}")
    "#,
        "10",
    );
}

#[test]
fn test_generic_struct_same_type_multiple_instances() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<T>
    value T

fn main()
    let a = Wrapper<int>(value: 1)
    let b = Wrapper<int>(value: 2)
    println(f"{a.value + b.value}")
    "#,
        "3",
    );
}
