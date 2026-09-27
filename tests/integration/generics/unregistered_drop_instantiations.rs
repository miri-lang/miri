// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic instantiation is released through its own drop thunk wherever a
//! lowered body holds a value of it, however the program reached the type:
//! written down, returned from a generic function, built inside one, nested in
//! another instantiation, or wrapped in an optional, a tuple or an enum
//! payload. A thunk chosen from anything short of every type a body holds
//! falls back to the shared one, which skips a field still written at a
//! parameter and leaks the managed value it stores.

use super::utils::*;

const BOX: &str = r#"
class Box<T>
    var value T

    fn init(value T)
        self.value = value
"#;

fn with_box(main: &str) -> String {
    format!("{BOX}\n{main}")
}

#[test]
fn an_instance_returned_from_a_generic_function_releases_its_field() {
    assert_runs_with_output(
        &with_box(
            r#"
fn wrap<T>(x T) Box<T>
    return Box<T>(x)

fn main()
    let b = wrap("a" + "b")
    println(b.value)
"#,
        ),
        "ab",
    );
}

#[test]
fn an_instance_built_only_inside_a_generic_function_releases_its_field() {
    assert_runs_with_output(
        &with_box(
            r#"
fn length_of<T>(x T) int
    let held = Box<T>(x)
    return 1

fn main()
    let n = length_of("a" + "b")
    println(f"{n}")
"#,
        ),
        "1",
    );
}

#[test]
fn a_nested_instance_from_a_generic_function_releases_every_level() {
    assert_runs_with_output(
        &with_box(
            r#"
fn wrap<T>(x T) Box<T>
    return Box<T>(x)

fn main()
    let b = wrap(wrap("a" + "b"))
    println(b.value.value)
"#,
        ),
        "ab",
    );
}

#[test]
fn an_optional_instance_from_a_generic_function_releases_its_field() {
    assert_runs_with_output(
        &with_box(
            r#"
fn maybe<T>(x T) Box<T>?
    return Box<T>(x)

fn main()
    let b = maybe("a" + "b")
    match b
        Some(inner): println(inner.value)
        None: println("none")
"#,
        ),
        "ab",
    );
}

#[test]
fn an_instance_in_a_tuple_from_a_generic_function_releases_its_field() {
    assert_runs_with_output(
        &with_box(
            r#"
fn paired<T>(x T) (Box<T>, int)
    return (Box<T>(x), 1)

fn main()
    let p = paired("a" + "b")
    let first = p.0
    println(first.value)
"#,
        ),
        "ab",
    );
}

#[test]
fn an_instance_in_a_generic_enum_payload_releases_its_field() {
    assert_runs_with_output(
        &with_box(
            r#"
enum Slot<T>
    Full(T)
    Empty

fn filled<T>(x T) Slot<Box<T>>
    return Slot.Full(Box<T>(x))

fn main()
    let s = filled("a" + "b")
    match s
        Slot.Full(inner): println(inner.value)
        Slot.Empty: println("empty")
"#,
        ),
        "ab",
    );
}
