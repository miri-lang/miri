// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A runtime parameter of a function type takes the closure and the address
//! of the trampoline the runtime calls it through, whichever runtime function
//! declares it. The runtime calls the closure in one shape only, so a
//! declaration that promises it any other is refused where it is written.

use super::utils::*;

#[test]
fn a_runtime_function_calls_a_closure_it_is_handed() {
    assert_runs_with_output(
        r#"
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

fn main()
    var l = List([3, 1, 2])
    miri_rt_list_sort_by(l, fn(a int, b int) int: b - a)
    println(f"{l[0]} {l[1]} {l[2]}")
"#,
        "3 2 1",
    );
}

/// A program declaring `miri_rt_list_sort_by` with `declaration` as its
/// parameter list and calling it with `closure`.
fn sort_by_declared(declaration: &str, closure: &str) -> String {
    format!(
        r#"
use system.collections.list

struct Pair
    a int
    b int

runtime "core" fn miri_rt_list_sort_by({declaration})

fn main()
    var l = List([3, 1, 2])
    miri_rt_list_sort_by(l, {closure})
    println(f"{{l[0]}}")
"#
    )
}

#[test]
fn a_closure_declared_with_another_arity_than_the_runtime_calls_is_refused() {
    assert_compiler_error(
        &sort_by_declared(
            "l [int], f fn(int, int, int) int",
            "fn(a int, b int, c int) int: a - b + c",
        ),
        "`miri_rt_list_sort_by` calls `f` with 2 arguments, but `f` is declared to take 3",
    );
}

#[test]
fn a_closure_declared_to_answer_other_than_int_is_refused() {
    for (answer, closure) in [
        ("i128", "fn(a int, b int) i128: (a - b) as i128"),
        ("i32", "fn(a int, b int) i32: (a - b) as i32"),
        ("Pair", "fn(a int, b int) Pair: Pair(a: a, b: b)"),
    ] {
        assert_compiler_error(
            &sort_by_declared(&format!("l [int], f fn(int, int) {answer}"), closure),
            &format!("`miri_rt_list_sort_by` reads what `f` answers as an `int`, but `f` is declared to answer `{answer}`"),
        );
    }
}

#[test]
fn a_closure_declared_over_other_values_than_the_list_holds_is_refused() {
    assert_compiler_error(
        &sort_by_declared(
            "l [int], f fn(i128, i128) int",
            "fn(a i128, b i128) int: (a - b) as int",
        ),
        "`miri_rt_list_sort_by` calls `f` with elements of `l`, which are `int`, but `f` takes `i128`",
    );
}

#[test]
fn a_function_typed_parameter_the_runtime_calls_nothing_through_is_refused() {
    assert_compiler_error(
        r#"
runtime "core" fn miri_rt_list_reverse(f fn() int)

fn main()
    miri_rt_list_reverse(fn() int: 1)
"#,
        "`miri_rt_list_reverse` calls no closure through `f`",
    );
}

#[test]
fn a_parameter_the_runtime_calls_as_a_closure_must_be_declared_as_a_function() {
    assert_compiler_error(
        &sort_by_declared("l [int], f int", "7"),
        "`miri_rt_list_sort_by` calls `f` as a closure, so it must be declared as a function",
    );
}

/// A closure that changes the list the runtime is sorting stops the program:
/// the sort holds the list's elements apart while it runs, and a list that is
/// not as the sort left it when the sort ends cannot be given them back.
#[test]
fn a_closure_that_grows_the_list_being_sorted_stops_the_program() {
    assert_runtime_error(
        r#"
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

class H
    var l List<int>

fn grow(h H, a int, b int) int
    h.l.push(a)
    return a - b

fn main()
    let h = H(l: List([5, 4, 3, 2, 1, 9, 8, 7]))
    miri_rt_list_sort_by(h.l, fn(a int, b int) int: grow(h, a, b))
    println(f"{h.l.length()} {h.l[0]}")
"#,
        "the list was changed by the closure it was being sorted with",
    );
}

/// A closure that replaces the list being sorted leaves its sorted elements
/// reaching no one, which stops the program the same way. The sort's own
/// reference keeps the list it was handed alive until then, so the guard sees
/// nothing read after it is freed.
#[test]
fn a_closure_that_replaces_the_list_being_sorted_stops_the_program() {
    let result = crate::utils::miri_run_with_env(
        r#"
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

class H
    var l List<int>

    fn replaced(a int, b int) int
        self.l = List([0])
        return a - b

fn main()
    let h = H(l: List([5, 4, 3, 2, 1, 9, 8, 7]))
    miri_rt_list_sort_by(h.l, fn(a int, b int) int: h.replaced(a, b))
    println(f"{h.l.length()} {h.l[0]}")
"#,
        "MIRI_HEAP_GUARD",
        "1",
    );
    let output = result.output();
    assert!(!result.success, "the program ran on: {output}");
    assert!(
        output.contains("the list was changed by the closure it was being sorted with"),
        "the sort did not stop the program: {output}"
    );
    assert!(
        !result.stderr.contains("MIRI_HEAP_GUARD:"),
        "the guard saw a freed block: {output}"
    );
}
