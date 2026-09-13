// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_method_element_at_out_of_bounds() {
    assert_compiler_error(
        r#"
use system.collections.array

let a = [1, 2, 3]
let x = a.element_at(5)
"#,
        "Index out of bounds",
    );
}

#[test]
fn test_array_method_element_at_valid_index() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [1, 2, 3]
let x = a.element_at(2)
println(f"{x}")
"#,
        "3",
    );
}

#[test]
fn test_array_method_element_at_negative() {
    assert_compiler_error(
        r#"
use system.collections.array

let a = [10, 20, 30]
let x = a.element_at(-1)
"#,
        "must be a non-negative integer",
    );
}

#[test]
fn test_array_method_set_out_of_bounds() {
    assert_compiler_error(
        r#"
use system.collections.array

let a = [1, 2, 3]
a.set(9, 0)
"#,
        "Index out of bounds",
    );
}

#[test]
fn test_tuple_method_element_at_out_of_bounds() {
    assert_compiler_error(
        r#"
use system.collections.tuple

let t = (1, 2, 3)
let x = t.element_at(7)
"#,
        "Index out of bounds",
    );
}

/// A List grows, so its length is not a compile-time property and a constant
/// index past its current end must still compile. The same index on a
/// fixed-size Array is rejected, which is what makes this a real distinction
/// rather than an accident of the index never being examined.
#[test]
fn test_list_method_no_bounds_check() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([1, 2, 3])
    match l.remove_at(5)
        Some(v)
            println(f"unexpected {v}")
        None
            println("no compile-time check")
    if l.length() > 100
        println(f"{l.get(5)}")
    println(f"{l.length()}")
"#,
        "no compile-time check\n3",
    );
}

#[test]
fn test_array_unknown_attribute_on_method() {
    assert_compiler_error(
        r#"
class Test
    @unknown_attr("index")
    public fn get(index int) int
        return 0
"#,
        "Unknown Attribute",
    );
}

#[test]
fn test_bounds_checked_attribute_on_wrong_target() {
    assert_compiler_error(
        r#"
@index_bounds_check("index")
enum MyEnum
    A
    B
"#,
        "not valid on",
    );
}

/// An index read out of a binding that can never be reassigned is as knowable
/// as the literal it was initialized from, including when the initializer is a
/// call to a function whose body is a constant. Both forms are rejected at
/// compile time rather than trapping at runtime.
#[test]
fn test_index_through_immutable_binding_of_constant_call() {
    assert_compiler_error(
        r#"
fn get_index() int
    5

var a = [1, 2, 3]
let i = get_index()
a[i] = 99
println(f"{a[0]}")
"#,
        "Index out of bounds: index 5 but collection has 3 elements",
    );
}

/// A `var` can be assigned after its declaration, so its initializer says
/// nothing about the value at the index site and no compile-time verdict is
/// possible. The runtime check is what catches this one.
#[test]
fn test_index_through_mutable_binding_is_not_rejected_at_compile_time() {
    assert_runs_with_output(
        r#"
var a = [1, 2, 3]
var i = 5
i = 1
a[i] = 99
println(f"{a[1]}")
"#,
        "99",
    );
}
