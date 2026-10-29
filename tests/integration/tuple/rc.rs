// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_alias_no_double_free() {
    assert_runs(
        r#"
let t1 = (1, 2, 3)
let t2 = t1
"#,
    );
}

#[test]
fn test_tuple_reassign() {
    assert_runs(
        r#"
var t = (1, 2, 3)
t = (4, 5, 6)
"#,
    );
}

#[test]
fn test_tuple_with_managed_types() {
    assert_runs(
        r#"
use system.collections.list

// If this crashes, there's a problem with tuple drop code.
// Tuples shouldn't leak memory (verified by leak sanitizer / Miri internal RC checks if any).
let t = (List([1, 2, 3]), "hello")
let l2 = t.0 // Increase RC
let s2 = t.1 // Increase RC
"#,
    );
}

#[test]
fn test_tuple_nested_managed_rc() {
    assert_runs(
        r#"
use system.collections.list
use system.collections.tuple

let t = (List([List([1])]), "outer")
let inner = t.0.element_at(0)
"#,
    );
}

/// Reading a managed element out of a tuple leaves it in the tuple, so a binding
/// that takes it holds a reference of its own and the tuple still releases its
/// element exactly once.
#[test]
fn test_tuple_managed_element_bound_by_let_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let t = (List([1, 2, 3]), f"s{1}")
    let l2 = t.0
    let s2 = t.1
    println(f"{l2.length()} {s2} {t.0.length()} {t.1}")
"#,
        "3 s1 3 s1",
    );
}

/// A managed tuple element passed straight to a function, or to one that
/// returns it, is not released by the call on the tuple's behalf.
#[test]
fn test_tuple_managed_element_passed_to_a_call_is_released_once() {
    assert_heap_guard_output(
        r#"
fn take(x String) String
    return x

fn main()
    var t = (f"a{1}", 1)
    println(t.0)
    let r = take(t.0)
    t = (f"b{2}", 2)
    println(r)
    println(t.0)
"#,
        "a1\na1\nb2",
    );
}

/// A program that evaluates `condition` a thousand times over the tuple
/// `t = (1, 2, 3)`, with `declarations` above `main`, and prints how often it
/// held. The one allocation such a program makes is the printed text.
fn thousand_calls(declarations: &str, condition: &str) -> String {
    format!(
        "use system.io\nuse system.collections.tuple\n{declarations}\nfn main()\n    let t = (1, 2, 3)\n    var s = 0\n    var i = 0\n    while i < 1000\n        if {condition}: s += 1\n        i += 1\n    println(f\"{{s}}\")\n"
    )
}

/// A tuple held as its bytes is its methods' receiver as it is: a call
/// allocates nothing, however often it is made.
#[test]
fn test_tuple_methods_on_a_held_tuple_allocate_nothing() {
    for condition in [
        "t.length() == 3",
        "t.contains(i % 5)",
        "(t.index_of(i % 5) ?? -1) >= 0",
        "(t.first() ?? 0) == 1",
        "(t.last() ?? 0) == 3",
        "not t.is_empty()",
        "t.element_at(i % 3) == 2",
    ] {
        assert_allocation_count(&thousand_calls("", condition), 1);
    }
}

#[test]
fn test_a_for_loop_over_a_held_tuple_allocates_nothing() {
    for condition in ["sum_of(t) == 6", "sum_of((1, 2, 3)) == 6"] {
        assert_allocation_count(
            &thousand_calls(
                "fn sum_of(t (int, int, int)) int\n    var s = 0\n    for v in t\n        s += v\n    return s\n",
                condition,
            ),
            1,
        );
    }
}

#[test]
fn test_a_tuple_method_on_a_parameter_allocates_nothing() {
    assert_allocation_count(
        &thousand_calls(
            "fn has(t (int, int, int), x int) bool\n    return t.contains(x) and not t.is_empty() and t.length() == 3\n",
            "has((i, 2, 3), 3)",
        ),
        1,
    );
}

#[test]
fn test_a_tuple_method_on_a_class_field_allocates_nothing_per_call() {
    // The instance is the one allocation beside the printed text.
    assert_allocation_count(
        r#"
use system.io
use system.collections.tuple

class Holder
    t (int, int, int)

fn main()
    let h = Holder(t: (1, 2, 3))
    var s = 0
    var i = 0
    while i < 1000
        if h.t.contains(i % 5) and h.t.length() == 3: s += 1
        i += 1
    println(f"{s}")
"#,
        2,
    );
}

/// No tuple held as its bytes is copied into an allocation for a method call:
/// the lowered `main` builds no tuple into an untyped pointer.
#[test]
fn test_a_tuple_method_call_builds_no_boxed_receiver() {
    use miri::ast::types::TypeKind;
    use miri::mir::{AggregateKind, Rvalue, StatementKind};
    let source = thousand_calls("", "t.contains(i % 5) and t.length() == 3");
    let bodies = miri::pipeline::Pipeline::new()
        .get_mir_bodies_with_rc(&source)
        .expect("pipeline should lower the program");
    let (_, main) = bodies
        .iter()
        .find(|(name, _)| name.ends_with("main"))
        .expect("a main body");
    let boxed = main
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| {
            matches!(
                &statement.kind,
                StatementKind::Assign(place, Rvalue::Aggregate(AggregateKind::Tuple(_), _))
                    if main.local_decls[place.local.0].ty.kind == TypeKind::RawPtr
            )
        });
    assert!(!boxed, "main boxes a tuple receiver:\n{main:?}");
}
