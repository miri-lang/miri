// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_requires_import_for_methods() {
    assert_compiler_error(
        r#"
let t = (1, 2, 3)
println(f"{t.length()}")
"#,
        "does not have members",
    );
}

#[test]
fn test_tuple_element_at_oob() {
    // OOB element_at triggers a runtime "Array index out of bounds" error.
    assert_runtime_crash(
        r#"
use system.collections.tuple
let t = (1, 2, 3)
let x = t.element_at(5)
"#,
    );
}

#[test]
fn test_tuple_access_oob_compiler_error() {
    assert_compiler_error(
        r#"
let t = (1, 2, 3)
let x = t.3
"#,
        "Tuple index out of bounds",
    );
}

#[test]
fn test_heterogeneous_tuple_methods_error() {
    // Methods in system.collections.tuple are defined for Tuple<T>.
    // A heterogeneous tuple (int, String) correctly does not support these methods.
    assert_compiler_error(
        r#"
use system.collections.tuple
let t = (1, "hello")
println(f"{t.contains(1)}")
"#,
        "does not have members",
    );
}

#[test]
fn test_tuple_mutation_restriction() {
    assert_compiler_error(
        r#"
let t = (1, 2)
t.0 = 10
"#,
        "Cannot assign to field of immutable variable",
    );
}

#[test]
fn test_a_tuple_element_cannot_be_assigned_by_index() {
    assert_compiler_error(
        r#"
var t = (1, 2)
t[0] = 5
"#,
        "a tuple's elements are fixed when it is built",
    );
}

#[test]
fn test_a_tuple_element_cannot_be_assigned_by_position() {
    assert_compiler_error(
        r#"
var t = (1, "a")
t.1 = "b"
"#,
        "a tuple's elements are fixed when it is built",
    );
}

#[test]
fn test_a_new_tuple_replaces_one_with_a_changed_element() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var t = (1, "a" + "b")
    let u = t
    t = (5, t.1)
    println(f"{t.0} {t.1} {u.0}")
"#,
        "5 ab 1",
    );
}

/// A `for` loop binds one variable at one type, so a tuple whose elements
/// differ in type cannot be walked: whichever element came first would be read
/// at the wrong type.
#[test]
fn test_a_for_loop_over_a_tuple_starting_with_a_number_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let t = (1, "a")
    for v in t
        println(f"{v}")
"#,
        "MER_TYP_083",
    );
}

#[test]
fn test_a_for_loop_over_a_tuple_starting_with_a_string_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    for v in ("a", 1)
        println(f"{v.length()}")
"#,
        "MER_TYP_083",
    );
}

/// Elements of different integer widths are laid out apart, so they are not
/// one element type either, even though each value would fit the other.
#[test]
fn test_a_for_loop_over_a_tuple_of_mixed_integer_widths_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let a i32 = 7
    for v in (a, 2, 3)
        println(f"{v}")
"#,
        "MER_TYP_083",
    );
}

/// An `i32` and an `int` are laid out at different widths, so a tuple mixing
/// them has no element type its methods could read every element at: it has
/// no methods, as any other mixed tuple, rather than failing to compile later.
#[test]
fn test_a_tuple_of_mixed_integer_widths_has_no_methods() {
    for call in ["t.contains(2)", "t.element_at(0)", "t.length()"] {
        assert_compiler_error(
            &format!(
                "use system.io\nuse system.collections.tuple\n\nfn main()\n    let a i32 = 7\n    let t = (a, 2, 3)\n    println(f\"{{{call}}}\")\n"
            ),
            "does not have members",
        );
    }
}

#[test]
fn test_a_tuple_of_mixed_integer_widths_is_indexed_only_by_a_literal() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let a i32 = 7
    let t = (a, 2, 3)
    var i = 1
    println(f"{t[i]}")
"#,
        "Tuple index must be an integer literal for heterogeneous tuples",
    );
}

#[test]
fn test_a_tuple_of_mixed_integer_widths_reads_each_element_by_position() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a i32 = 7
    let t = (a, 2, 3)
    println(f"{t.0} {t.1} {t.2}")
"#,
        "7 2 3",
    );
}

#[test]
fn test_a_tuple_of_mixed_integer_widths_cannot_be_sliced() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let a i32 = 7
    let t = (a, 2, 3)
    let s = t[0..2]
    println(f"{s.length()}")
"#,
        "Cannot slice heterogeneous tuple",
    );
}
