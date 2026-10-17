// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The list methods that hand an element back as an optional: `pop`,
//! `remove_at`, and the `Stack` and `Queue` methods built on them.
//!
//! An optional whose payload is a reference-counted pointer is laid out
//! differently from one boxing a scalar, so a body compiled once for every
//! element type cannot hand one back at both. The selection tests pin which
//! instantiations reach a body of their own; the value tests run those bodies
//! under the leak check, the MIR verifier and the heap guard, with class
//! elements and with strings built at run time.

use super::utils::*;
use miri::pipeline::Pipeline;

/// The link names of every body `source` lowers to.
fn body_names(source: &str) -> Vec<String> {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("pipeline should lower the program")
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Asserts that a body whose link name ends in `suffix` was lowered.
fn assert_lowers_body(names: &[String], suffix: &str) {
    assert!(
        names.iter().any(|name| name.ends_with(suffix)),
        "no body named *{suffix} among {names:?}"
    );
}

const NODE_REMOVALS: &str = r#"
use system.io
use system.collections.list
use system.collections.stack
use system.collections.queue

class Node
    var value int

fn value_of(node Node?) int
    match node
        Some(n): return n.value
        None: return -1

fn main()
    var xs = List<Node>()
    xs.push(Node(value: 1))
    xs.push(Node(value: 2))
    let a = xs.pop()
    let b = xs.remove_at(0)
    var st = Stack<Node>()
    st.push(Node(value: 3))
    let c = st.pop()
    var q = Queue<Node>()
    q.enqueue(Node(value: 4))
    let d = q.dequeue()
    var ns = List<int>()
    ns.push(5)
    let e = ns.pop()
    println(f"{value_of(a)} {value_of(b)} {value_of(c)} {value_of(d)} {e ?? 0}")
"#;

/// At a class element, `pop` and `remove_at` hand back an optional holding a
/// pointer, so they reach bodies compiled for that element, and so do the
/// `Stack` and `Queue` bodies that call them.
#[test]
fn test_pop_and_remove_at_on_a_class_list_reach_bodies_of_their_own() {
    let names = body_names(NODE_REMOVALS);
    assert_lowers_body(&names, "List$Node.pop");
    assert_lowers_body(&names, "List$Node.remove_at");
    assert_lowers_body(&names, "Stack$Node.pop");
    assert_lowers_body(&names, "Queue$Node.dequeue");
}

/// At an `int` element the optional boxes its payload in every body, so the
/// shared `pop` still serves it.
#[test]
fn test_pop_on_an_int_list_keeps_the_shared_body() {
    let names = body_names(NODE_REMOVALS);
    assert!(
        !names.iter().any(|name| name.ends_with("List$int.pop")),
        "List<int>.pop got a body of its own: {names:?}"
    );
    assert_lowers_body(&names, "miri.List.pop");
}

#[test]
fn test_pop_and_remove_at_hand_back_class_elements() {
    assert_runs_with_output(NODE_REMOVALS, "2 1 3 4 5");
}

#[test]
fn test_pop_and_remove_at_hand_back_class_elements_under_the_heap_guard() {
    assert_heap_guard_output(NODE_REMOVALS, "2 1 3 4 5");
}

const STRING_REMOVALS: &str = r#"
use system.io
use system.collections.list

fn main()
    var xs = List<String>()
    let n = 7
    xs.push("a" + "b")
    xs.push(f"n{n}")
    xs.push("c" + "d")
    xs.push(f"m{n + 1}")
    let last = xs.pop()
    let first = xs.remove_at(0)
    let past = xs.remove_at(9)
    match last
        Some(s): println(s)
        None: println("none")
    match first
        Some(s): println(s)
        None: println("none")
    match past
        Some(s): println(s)
        None: println("none")
    println(f"{xs.length()} {xs[0]} {xs[1]}")
"#;

#[test]
fn test_pop_and_remove_at_hand_back_strings_built_at_run_time() {
    assert_runs_with_output(STRING_REMOVALS, "m8\nab\nnone\n2 n7 cd");
}

#[test]
fn test_pop_and_remove_at_hand_back_strings_under_the_heap_guard() {
    assert_heap_guard_output(STRING_REMOVALS, "m8\nab\nnone\n2 n7 cd");
}

const EMPTY_AND_KEPT: &str = r#"
use system.io
use system.collections.list

class Node
    var value int

fn value_of(node Node?) int
    match node
        Some(n): return n.value
        None: return -1

fn main()
    var xs = List<Node>()
    let none = xs.pop()
    println(f"{value_of(none)}")
    let kept = Node(value: 9)
    xs.push(kept)
    let popped = xs.pop()
    println(f"{value_of(popped)} {kept.value} {xs.length()}")
    var dropped = List<Node>()
    dropped.push(Node(value: 1))
    dropped.pop()
    dropped.push(Node(value: 2))
    dropped.remove_at(0)
    println(f"{dropped.length()}")
"#;

/// Popping an empty list hands back `None`; an element still bound outside
/// the list survives its removal; a result nobody binds is released.
#[test]
fn test_removal_from_a_class_list_keeps_outside_references_and_releases_discarded_results() {
    assert_runs_with_output(EMPTY_AND_KEPT, "-1\n9 9 0\n0");
}

#[test]
fn test_removal_from_a_class_list_under_the_heap_guard() {
    assert_heap_guard_output(EMPTY_AND_KEPT, "-1\n9 9 0\n0");
}

const STACK_AND_QUEUE: &str = r#"
use system.io
use system.collections.stack
use system.collections.queue

class Node
    var value int

fn value_of(node Node?) int
    match node
        Some(n): return n.value
        None: return -1

fn main()
    var st = Stack<Node>()
    st.push(Node(value: 1))
    st.push(Node(value: 2))
    let top = st.peek()
    let popped = st.pop()
    println(f"{value_of(top)} {value_of(popped)} {st.length()}")
    var q = Queue<String>()
    let n = 3
    q.enqueue("x" + "y")
    q.enqueue(f"z{n}")
    let front = q.peek()
    let out = q.dequeue()
    match front
        Some(s): println(s)
        None: println("none")
    match out
        Some(s): println(s)
        None: println("none")
    println(f"{q.length()}")
"#;

#[test]
fn test_stack_and_queue_hand_back_managed_elements() {
    assert_runs_with_output(STACK_AND_QUEUE, "2 2 1\nxy\nxy\n1");
}

#[test]
fn test_stack_and_queue_hand_back_managed_elements_under_the_heap_guard() {
    assert_heap_guard_output(STACK_AND_QUEUE, "2 2 1\nxy\nxy\n1");
}

const FIRST_AND_LAST: &str = r#"
use system.io
use system.collections.list

class Node
    var value int

fn value_of(node Node?) int
    match node
        Some(n): return n.value
        None: return -1

fn main()
    var xs = List<Node>()
    xs.push(Node(value: 1))
    xs.push(Node(value: 2))
    println(f"{value_of(xs.first())} {value_of(xs.last())} {xs.length()}")
"#;

#[test]
fn test_first_and_last_on_a_class_list_under_the_heap_guard() {
    assert_heap_guard_output(FIRST_AND_LAST, "1 2 2");
}

const OPTIONAL_ELEMENTS: &str = r#"
use system.io
use system.collections.list

class Node
    var value int

fn main()
    var xs = List<Node?>()
    xs.push(Some(Node(value: 1)))
    xs.push(None)
    xs.push(Some(Node(value: 3)))
    let last = xs.pop()
    let first = xs.remove_at(0)
    match last
        Some(inner)
            match inner
                Some(n): println(f"{n.value}")
                None: println("inner none")
        None: println("none")
    match first
        Some(inner)
            match inner
                Some(n): println(f"{n.value}")
                None: println("inner none")
        None: println("none")
    println(f"{xs.length()}")
"#;

/// An element that is itself optional is reference-counted, so its removals
/// reach bodies compiled for it as well.
#[test]
fn test_pop_on_a_list_of_optionals_reaches_a_body_of_its_own() {
    let names = body_names(OPTIONAL_ELEMENTS);
    assert!(
        names
            .iter()
            .any(|name| name.contains("List$") && name.ends_with(".pop")),
        "List<Node?>.pop has no body of its own: {names:?}"
    );
}

#[test]
fn test_pop_and_remove_at_hand_back_optional_elements_under_the_heap_guard() {
    assert_heap_guard_output(OPTIONAL_ELEMENTS, "3\n1\n1");
}
