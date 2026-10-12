// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A parameter guard is checked on entry with the operator it spells, and a
//! call whose argument fails it ends the program with MER_RT_004 and a
//! sentence naming the function, the parameter and the guard, before the
//! body runs.

use super::utils::*;

/// A program calling `f` with `argument`, after a call that passes.
fn guarded(declarations: &str, passing: &str, argument: &str) -> String {
    format!(
        "use system.collections.set\nuse system.collections.list\n\n{declarations}\n\nfn main()\n    f({passing})\n    f({argument})\n"
    )
}

const IN_SET: &str = "fn f(a int in {1, 2, 3})\n    println(f\"ran {a}\")";
const IN_ARRAY: &str = "fn f(a int in [1, 2, 3])\n    println(f\"ran {a}\")";
const NOT_IN_ARRAY: &str = "fn f(a int not in [1, 2, 3])\n    println(f\"ran {a}\")";
const NOT_ZERO: &str = "fn f(a int not 0)\n    println(f\"ran {a}\")";
const IN_LIST_OF_STRINGS: &str = "fn f(a String in List(['x', 'y']))\n    println(f\"ran {a}\")";

/// The program stops with `sentence`; the code it reports, MER_RT_004, is
/// pinned by the conformance fixture.
fn assert_guard_refused(program: &str, sentence: &str) {
    assert_runtime_error(program, sentence);
}

#[test]
fn a_greater_than_guard_stops_with_the_guard_named() {
    assert_guard_refused(
        "fn pos(x int > 0)\n    println(f\"ran {x}\")\n\nfn main()\n    pos(1)\n    pos(0)\n",
        "pos: x must be > 0",
    );
}

#[test]
fn an_in_guard_over_a_set_admits_a_member() {
    assert_runs_with_output(&guarded(IN_SET, "2", "3"), "ran 2\nran 3");
}

#[test]
fn an_in_guard_over_a_set_stops_a_non_member() {
    assert_guard_refused(&guarded(IN_SET, "2", "5"), "f: a must be in {1, 2, 3}");
}

#[test]
fn an_in_guard_over_an_array_admits_a_member() {
    assert_runs_with_output(&guarded(IN_ARRAY, "1", "3"), "ran 1\nran 3");
}

#[test]
fn an_in_guard_over_an_array_stops_a_non_member() {
    assert_guard_refused(&guarded(IN_ARRAY, "1", "5"), "f: a must be in [1, 2, 3]");
}

#[test]
fn a_not_in_guard_admits_a_non_member() {
    assert_runs_with_output(&guarded(NOT_IN_ARRAY, "4", "5"), "ran 4\nran 5");
}

#[test]
fn a_not_in_guard_stops_a_member() {
    assert_guard_refused(
        &guarded(NOT_IN_ARRAY, "4", "2"),
        "f: a must not be in [1, 2, 3]",
    );
}

#[test]
fn a_not_guard_admits_another_value() {
    assert_runs_with_output(&guarded(NOT_ZERO, "1", "-1"), "ran 1\nran -1");
}

#[test]
fn a_not_guard_stops_the_value_it_names() {
    assert_guard_refused(&guarded(NOT_ZERO, "1", "0"), "f: a must not be 0");
}

#[test]
fn an_in_guard_over_a_list_of_strings_compares_text() {
    assert_heap_guard_output(&guarded(IN_LIST_OF_STRINGS, "'x'", "'y'"), "ran x\nran y");
}

#[test]
fn an_in_guard_over_a_list_of_strings_stops_other_text() {
    assert_guard_refused(
        &guarded(IN_LIST_OF_STRINGS, "'x'", "'z'"),
        "f: a must be in List([\"x\", \"y\"])",
    );
}
