// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic enum's payload is bound, rendered, stored and released at one
//! type: the declared payload with the instance's arguments substituted all
//! the way through, a nullable argument standing for the optional it denotes.
//! A binder reading the payload at a different type than the drop path
//! releases it at leaks it or frees it twice, so each program runs under the
//! heap guard and checks the value it reads back.

use super::super::utils::*;

const BOX: &str = r#"
class Box<T>
    var value T

    fn init(value T)
        self.value = value
"#;

#[test]
fn a_payload_nesting_the_parameter_outlives_the_enum_it_was_bound_from() {
    assert_heap_guard_output(
        &format!(
            r#"{BOX}
enum Slot<T>
    Full(Box<T>)
    Empty

fn main()
    var s Slot<String> = Slot.Full(Box<String>("x" + "y"))
    match s
        Slot.Full(b):
            s = Slot.Empty
            println(b.value)
        Slot.Empty: println("empty")
"#
        ),
        "xy",
    );
}

#[test]
fn a_nullable_managed_payload_outlives_the_enum_it_was_bound_from() {
    assert_heap_guard_output(
        r#"
enum Slot<T>
    Full(T)
    Empty

fn main()
    var n String? = None
    n = "a" + "bc"
    var s Slot<String?> = Slot.Full(n)
    match s
        Slot.Full(v):
            s = Slot.Empty
            match v
                Some(t): println(t)
                None: println("none")
        Slot.Empty: println("empty")
"#,
        "abc",
    );
}

#[test]
fn a_nullable_wide_scalar_payload_reads_back_whole() {
    // A 128-bit value prints only its low word, so the check compares it.
    assert_heap_guard_output(
        r#"
enum Slot<T>
    Full(T)
    Empty

fn main()
    let big i128 = 9223372036854775807
    var n i128? = None
    n = big * 1000
    let s Slot<i128?> = Slot.Full(n)
    match s
        Slot.Full(v):
            match v
                Some(t): println(f"{t == big * 1000}")
                None: println("none")
        Slot.Empty: println("empty")
"#,
        "true",
    );
}

#[test]
fn a_nullable_managed_payload_renders_through_an_f_string() {
    assert_heap_guard_output(
        r#"
enum Slot<T>
    Full(T)
    Empty

fn main()
    var n String? = None
    n = "a" + "bc"
    let s Slot<String?> = Slot.Full(n)
    println(f"{s}")
"#,
        "Full(Some(abc))",
    );
}
