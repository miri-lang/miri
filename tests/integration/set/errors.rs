// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_lowercase_not_recognized() {
    // Lowercase `set` is a type annotation (like `{int}`), not a class constructor.
    // Using it as a constructor should fail.
    assert_compiler_error(
        "
let s = set()
",
        "Undefined",
    );
}

#[test]
fn test_set_requires_import_for_methods() {
    // Sets are now implicitly available (loaded via prelude).
    // This test verifies that Set methods work without explicit import.
    assert_runs_with_output(
        r#"
let s = {1, 2, 3}
println(f"{s.length()}")
"#,
        "3",
    );
}

#[test]
fn test_set_constructor_rejects_non_literal_arg() {
    // `Set(non-literal)` is not supported because lowering delegates to the set-literal
    // lowering. Passing an arbitrary value of `Set<T>` would silently produce an empty set.
    assert_compiler_error(
        "
use system.collections.set

let other = {1, 2, 3}
let s = Set(other)
",
        "only accepts a set literal",
    );
}

#[test]
fn set_with_type_argument_rejects_scalar_arg() {
    // Writing the element type must not skip the argument check. Lowering only
    // reads the argument when it is a set literal, so anything else is accepted
    // here, dropped there, and never mentioned.
    assert_compiler_error(
        "
use system.collections.set

fn main()
    let s = Set<int>(5)
    println(f\"{s.length()}\")
",
        "expects a set literal of 'int'",
    );
}

#[test]
fn set_with_type_argument_rejects_mismatched_element_type() {
    assert_compiler_error(
        "
use system.collections.set

fn main()
    let s = Set<int>({\"a\", \"b\"})
    println(f\"{s.length()}\")
",
        "expects a set literal of 'int'",
    );
}

#[test]
fn set_with_type_argument_accepts_a_matching_literal() {
    assert_runs_with_output(
        "
use system.collections.set

fn main()
    let s = Set<int>({1, 2})
    println(f\"{s.length()}\")
",
        "2",
    );
}

#[test]
fn set_with_type_argument_accepts_an_empty_brace_literal() {
    // `{}` names no elements, so it reads as an empty map and is equally the
    // contents of an empty set. It is a spelling that already existed, and the
    // argument check has to keep accepting it.
    assert_runs_with_output(
        "
use system.collections.set

fn main()
    var s = Set<f64>({})
    s.add(1.5)
    println(f\"{s.length()}\")
",
        "1",
    );
}

/// A set matches its elements by `==`, which cannot compare two functions,
/// so a function type is refused as an element, constructed or declared.
#[test]
fn a_function_is_refused_as_a_set_element() {
    assert_compiler_error(
        r#"
use system.collections.set

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    var s = Set<fn(int) int>()
    s.add(adder(10))
"#,
        "cannot be a Set element, which is matched by `==`",
    );
    assert_compiler_error(
        r#"
use system.collections.set

fn main()
    let s Set<fn(int) int> = Set<fn(int) int>()
"#,
        "MER_TYP_002",
    );
}

/// A tuple is compared member by member, so one holding a function is no
/// more comparable than the function is, and is refused as a set element.
#[test]
fn a_tuple_holding_a_function_is_refused_as_a_set_element() {
    let program = r#"
use system.collections.set

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    var s = Set<(int, fn(int) int)>()
    s.add((1, adder(10)))
"#;
    assert_compiler_error(program, "MER_TYP_002");
    assert_compiler_error(program, "cannot be a Set element, which is matched by `==`");
}

/// A function nested in a tuple inside a tuple is found as well.
#[test]
fn a_tuple_nesting_a_function_is_refused_as_a_set_element() {
    let program = r#"
use system.collections.set

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    var s = Set<((int, fn(int) int), String)>()
    s.add(((1, adder(10)), "a"))
"#;
    assert_compiler_error(program, "MER_TYP_002");
    assert_compiler_error(program, "function values cannot be compared with `==`");
}

/// A list compares its elements with `==`, so a tuple holding a list of
/// functions is no more comparable than the functions are.
#[test]
fn a_tuple_holding_a_list_of_functions_is_refused_as_a_set_element() {
    let program = r#"
use system.collections.set
use system.collections.list

fn main()
    let f = fn(x int) int: x + 1
    var s = Set<(int, List<fn(int) int>)>()
    s.add((1, List([f])))
"#;
    assert_compiler_error(program, "MER_TYP_002");
    assert_compiler_error(program, "cannot be a Set element, which is matched by `==`");
}

/// A struct is compared field by field, so one holding a list of functions
/// is refused as a set element.
#[test]
fn a_struct_holding_a_list_of_functions_is_refused_as_a_set_element() {
    let program = r#"
use system.collections.set
use system.collections.list

struct Holder
    n int
    fs List<fn(int) int>

fn main()
    let f = fn(x int) int: x + 1
    var s = Set<Holder>()
    s.add(Holder(n: 1, fs: List([f])))
"#;
    assert_compiler_error(program, "MER_TYP_002");
    assert_compiler_error(program, "cannot be a Set element, which is matched by `==`");
}

/// A class without an `equals` of its own is matched by identity, so what it
/// holds does not matter: one holding a list of functions is still accepted.
#[test]
fn a_class_holding_a_list_of_functions_is_matched_by_identity() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set
use system.collections.list

class Holder
    var fs List<fn(int) int>

fn main()
    let f = fn(x int) int: x + 1
    let a = Holder(fs: List([f]))
    let b = Holder(fs: List([f]))
    var s = Set<Holder>()
    s.add(a)
    s.add(a)
    s.add(b)
    println(f"{s.length()} {s.contains(a)}")
"#,
        "2 true",
    );
}
