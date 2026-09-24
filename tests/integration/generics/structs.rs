// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// --- Acceptance criteria tests ---

#[test]
fn test_generic_struct_int_construction_and_field_access() {
    assert_runs_with_output(
        r#"

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

/// A generic struct's field is stored at the type the struct is instantiated
/// with, whatever the field reads through: a float keeps its fraction, and a
/// 128-bit value keeps its high word.
#[test]
fn test_generic_struct_fields_keep_their_instantiated_width() {
    assert_runs_with_output(
        r#"
struct H<U>
    v U

fn held_wide(w i128) i128
    var h = H<i128>(v: w)
    return h.v

fn main()
    let a f32 = 1.5
    let d float = 0.5
    let w i128 = -18446744073709551621
    var h = H<f32>(v: a)
    let y = h.v
    let hd = H<float>(v: d)
    let z = hd.v
    let back = held_wide(w)
    println(f"{y} {z} {back == w} {back == -5}")
"#,
        "1.5 0.5 true false",
    );
}

/// A generic struct instantiated at a reference-counted type retains the value
/// it is built with and releases the one a field write replaces, reading the
/// field declared at the parameter at the instance's type argument.
#[test]
fn test_generic_struct_field_of_a_managed_type_is_counted() {
    assert_heap_guard_output(
        r#"
struct H<U>
    v U

fn swapped(a Option<int>, b Option<int>) Option<int>
    var h = H<Option<int>>(v: a)
    h.v = b
    return h.v

fn main()
    let a Option<int> = Some(1)
    let b Option<int> = Some(2)
    let name = "x" + "y"
    var hs = H<String>(v: name)
    hs.v = "z" + "w"
    println(f"{swapped(a, b) ?? 0} {hs.v} {name}")
"#,
        "2 zw xy",
    );
}
