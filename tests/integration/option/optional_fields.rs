// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Assigning into a field whose declared type is an optional.
//!
//! A bare value stored into a `T?` field has to arrive as `Some(value)`. Every
//! assignment below hands the field a bare payload, so each test fails the same
//! way if the store keeps the raw value: the read then treats the payload as a
//! pointer to an optional and faults. The tests pin the printed value rather
//! than only that the program ran.

use super::utils::*;

#[test]
fn bare_int_assigned_to_a_class_field_reads_back_as_some() {
    assert_runs_with_output(
        r#"
class Holder
    var v int?

fn main()
    var h = Holder(v: None)
    h.v = 5
    println(f"{h.v}")
"#,
        "Some(5)",
    );
}

#[test]
fn bare_int_assigned_to_a_struct_field_reads_back_as_some() {
    assert_runs_with_output(
        r#"
struct Holder
    v int?

fn main()
    var h = Holder(v: None)
    h.v = 5
    println(f"{h.v}")
"#,
        "Some(5)",
    );
}

#[test]
fn a_named_local_and_a_temporary_both_wrap_into_a_string_field() {
    assert_runs_with_output(
        r#"
class Holder
    var v String?

fn main()
    var kept = Holder(v: None)
    let s = "kept"
    kept.v = s
    var made = Holder(v: None)
    made.v = "made " + "here"
    println(f"{kept.v} {made.v} {s}")
"#,
        "Some(kept) Some(made here) kept",
    );
}

#[test]
fn an_already_optional_value_assigned_to_a_field_is_not_wrapped_twice() {
    assert_runs_with_output(
        r#"
class Holder
    var v int?

fn main()
    var h = Holder(v: None)
    h.v = Some(5)
    println(f"{h.v}")
    h.v = None
    println(f"{h.v}")
"#,
        "Some(5)\nNone",
    );
}

#[test]
fn reassigning_an_optional_field_releases_the_old_payload() {
    assert_heap_guard_output(
        r#"
struct Holder
    v String?

fn main()
    var h = Holder(v: None)
    h.v = "first"
    println(f"{h.v}")
    h.v = "second"
    println(f"{h.v}")
    let s = "third"
    h.v = s
    println(f"{h.v} {s}")
"#,
        "Some(first)\nSome(second)\nSome(third) third",
    );
}

#[test]
fn assigning_a_mismatched_payload_to_an_optional_field_is_rejected() {
    assert_compiler_error(
        r#"
class Holder
    var v int?

fn main()
    var h = Holder(v: None)
    h.v = "one"
"#,
        "cannot assign String to int?",
    );
}

#[test]
fn a_method_assigning_into_its_own_optional_field_wraps() {
    assert_runs_with_output(
        r#"
class Holder
    var v int?
    public fn fill()
        self.v = 5

fn main()
    var h = Holder(v: None)
    h.fill()
    println(f"{h.v}")
"#,
        "Some(5)",
    );
}

#[test]
fn a_nested_optional_field_wraps_the_stored_value() {
    assert_runs_with_output(
        r#"
struct Inner
    v String?

struct Outer
    inner Inner

fn main()
    var o = Outer(inner: Inner(v: None))
    o.inner.v = "deep"
    println(f"{o.inner.v}")
"#,
        "Some(deep)",
    );
}

#[test]
fn a_call_result_assigned_to_an_optional_field_wraps() {
    assert_runs_with_output(
        r#"
class Holder
    var v String?

fn make() String
    return "made"

fn main()
    var h = Holder(v: None)
    h.v = make()
    println(f"{h.v}")
"#,
        "Some(made)",
    );
}

#[test]
fn a_field_declared_as_option_of_t_wraps_like_one_spelled_t_question() {
    assert_runs_with_output(
        r#"
class Holder
    var v Option<int>

fn main()
    var h = Holder(v: None)
    h.v = 5
    println(f"{h.v}")
"#,
        "Some(5)",
    );
}

#[test]
fn an_optional_of_an_optional_field_keeps_both_layers() {
    assert_runs_with_output(
        r#"
class Holder
    var v Option<int?>

fn main()
    var h = Holder(v: None)
    h.v = Some(5)
    println(f"{h.v}")
"#,
        "Some(Some(5))",
    );
}

#[test]
fn an_optional_field_of_a_generic_class_wraps_its_type_argument() {
    assert_runs_with_output(
        r#"
class Box<T>
    var v T?
    public fn set(x T)
        self.v = x

fn main()
    var b = Box<int>(v: None)
    b.set(7)
    println(f"{b.v}")
"#,
        "Some(7)",
    );
}

#[test]
fn repeated_assignment_in_a_loop_keeps_the_payload_alive() {
    assert_heap_guard_output(
        r#"
class Holder
    var v String?

fn main()
    var h = Holder(v: None)
    var i = 0
    while i < 3
        h.v = f"round {i}"
        println(f"{h.v}")
        i = i + 1
"#,
        "Some(round 0)\nSome(round 1)\nSome(round 2)",
    );
}

#[test]
fn copying_one_optional_field_into_another_leaves_both_readable() {
    assert_heap_guard_output(
        r#"
class Holder
    var v String?

fn main()
    var a = Holder(v: None)
    var b = Holder(v: None)
    a.v = "shared"
    b.v = a.v
    println(f"{a.v} {b.v}")
"#,
        "Some(shared) Some(shared)",
    );
}
