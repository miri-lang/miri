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
