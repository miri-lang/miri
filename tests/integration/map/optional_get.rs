// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `Map.get`, which hands a value back as an optional.
//!
//! An optional whose payload is a reference-counted pointer is laid out
//! differently from one boxing a scalar, so a body compiled once for every
//! value type cannot hand one back at both. The selection test pins which
//! instantiations reach a body of their own; the value tests run those bodies
//! under the leak check, the MIR verifier and the heap guard, with class values
//! and with strings built at run time.

use super::utils::*;
use miri::pipeline::Pipeline;

const NODE_LOOKUPS: &str = r#"
use system.io
use system.collections.map

class Node
    var value int

fn value_of(node Node?) int
    match node
        Some(n): return n.value
        None: return -1

fn main()
    var m = Map<String, Node>()
    m.set("a", Node(value: 4))
    let kept = Node(value: 5)
    m.set("b", kept)
    let hit = m.get("a")
    let again = m.get("a")
    let miss = m.get("z")
    m.get("b")
    var counts = Map<int, int>()
    counts.set(1, 7)
    let n = counts.get(1)
    let b = m.get("b")
    println(f"{value_of(hit)} {value_of(again)} {value_of(miss)} {value_of(b)} {kept.value} {m.length()} {n ?? 0}")
"#;

/// At a class value `get` hands back an optional holding a pointer, so it
/// reaches a body compiled for that value; a map of plain integers still
/// shares one body.
#[test]
fn test_get_on_a_class_valued_map_reaches_a_body_of_its_own() {
    let names: Vec<String> = Pipeline::new()
        .get_mir_bodies_with_rc(NODE_LOOKUPS)
        .expect("pipeline should lower the program")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        names
            .iter()
            .any(|name| name.contains("Map$") && name.ends_with("Node.get")),
        "Map<String, Node>.get has no body of its own: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.contains("Map$int") && name.ends_with(".get")),
        "Map<int, int>.get got a body of its own: {names:?}"
    );
}

#[test]
fn test_get_hands_back_class_values() {
    assert_runs_with_output(NODE_LOOKUPS, "4 4 -1 5 5 2 7");
}

#[test]
fn test_get_hands_back_class_values_under_the_heap_guard() {
    assert_heap_guard_output(NODE_LOOKUPS, "4 4 -1 5 5 2 7");
}

const STRING_LOOKUPS: &str = r#"
use system.io
use system.collections.map

fn main()
    var m = Map<String, String>()
    let n = 2
    m.set("k" + "1", "v" + "1")
    m.set(f"k{n}", f"v{n}")
    let one = m.get("k1")
    let two = m.get(f"k{n}")
    let none = m.get("k3")
    match one
        Some(s): println(s)
        None: println("none")
    match two
        Some(s): println(s)
        None: println("none")
    match none
        Some(s): println(s)
        None: println("none")
    m.get("k1")
    let held = m["k1"]
    println(f"{m.length()} {held}")
"#;

#[test]
fn test_get_hands_back_strings_built_at_run_time() {
    assert_runs_with_output(STRING_LOOKUPS, "v1\nv2\nnone\n2 v1");
}

#[test]
fn test_get_hands_back_strings_under_the_heap_guard() {
    assert_heap_guard_output(STRING_LOOKUPS, "v1\nv2\nnone\n2 v1");
}
