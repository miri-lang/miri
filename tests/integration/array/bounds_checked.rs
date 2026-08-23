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
