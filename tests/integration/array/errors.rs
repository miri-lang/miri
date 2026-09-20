// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_index_out_of_bounds_literal() {
    assert_compiler_error(
        r#"
let a = [1, 2, 3]
let x = a[5]
    "#,
        "Index out of bounds",
    );
}

#[test]
fn test_array_mixed_types() {
    assert_compiler_error(
        r#"
let a = [1, "hello"]
    "#,
        "Array elements must have the same type",
    );
}

#[test]
fn test_array_non_int_index() {
    assert_compiler_error(
        r#"
let a = [1, 2, 3]
let x = a["x"]
    "#,
        "Array index must be an integer",
    );
}

/// An index reached through a binding that can never be reassigned carries the
/// constant its initializer folded to, so the read is judged where it is
/// written instead of trapping when the program runs.
#[test]
fn test_array_index_through_immutable_binding_is_a_compile_error() {
    assert_compiler_error(
        r#"

fn get_index() int
    5

fn main()
    let a = [1, 2, 3]
    let i = get_index()
    println(f"{a[i]}")
    "#,
        "Index out of bounds: index 5 but collection has 3 elements",
    );
}

#[test]
fn test_array_index_assignment_through_immutable_binding_is_a_compile_error() {
    assert_compiler_error(
        r#"

fn get_index() int
    5

fn main()
    var a = [1, 2, 3]
    let i = get_index()
    a[i] = 99
    println(f"{a[0]}")
    "#,
        "Index out of bounds: index 5 but collection has 3 elements",
    );
}

/// A `var` index can hold a different value at the read than at its
/// declaration, so no compile-time verdict is possible and the runtime bounds
/// check is what stops the read. This is the path that keeps the runtime guard
/// covered now that the constant forms are rejected earlier.
#[test]
fn test_array_index_through_mutable_binding_traps_at_runtime() {
    assert_runtime_error(
        r#"

fn main()
    let a = [1, 2, 3]
    var i = 0
    i = 5
    println(f"{a[i]}")
    "#,
        "Runtime error: Array index out of bounds",
    );
}

#[test]
fn test_array_index_assignment_through_mutable_binding_traps_at_runtime() {
    assert_runtime_error(
        r#"

fn main()
    var a = [1, 2, 3]
    var i = 0
    i = 5
    a[i] = 99
    println(f"{a[0]}")
    "#,
        "Runtime error: Array index out of bounds",
    );
}

#[test]
fn test_array_negative_index_runtime() {
    assert_runtime_error(
        r#"

fn get_index() int
    -1

fn main()
    let a = [10, 20, 30]
    let i = get_index()
    println(f"{a[i]}")
    "#,
        "Runtime error: Array index out of bounds",
    );
}

#[test]
fn array_with_type_arguments_rejects_a_wrong_argument_count() {
    // The constructor takes either nothing or one argument per element. One
    // argument for three elements is neither, and used to be accepted here and
    // refused only once MIR lowering was reached — late, and reported as an
    // internal failure of the compiler rather than as something the source got
    // wrong.
    assert_compiler_error(
        "
use system.collections.array

fn main()
    let a = Array<int, 3>(5)
    println(f\"{a.length()}\")
",
        "takes either no argument or exactly 3 of them",
    );
}

#[test]
fn array_with_type_arguments_still_constructs_when_empty() {
    assert_runs_with_output(
        "
use system.collections.array

fn main()
    let a = Array<int, 3>()
    println(f\"{a.length()}\")
",
        "3",
    );
}

#[test]
fn every_builtin_collection_constructor_checks_its_argument_with_type_arguments() {
    // The class gate. Writing type arguments must not weaken the positional
    // argument check for any of them — the defect was found one constructor at
    // a time, and this is what stops the next one being found the same way.
    for source in [
        "use system.collections.list\n\nfn main()\n    let c = List<int>(5)\n    println(f\"{c.length()}\")\n",
        "use system.collections.set\n\nfn main()\n    let c = Set<int>(5)\n    println(f\"{c.length()}\")\n",
        "use system.collections.map\n\nfn main()\n    let c = Map<int, int>(5)\n    println(f\"{c.length()}\")\n",
        "use system.collections.array\n\nfn main()\n    let c = Array<int, 3>(5)\n    println(f\"{c.length()}\")\n",
    ] {
        assert_compiler_error(source, "");
    }
}
