// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which list instantiations share the body compiled once for every element
//! type, and which reach one of their own.
//!
//! The shared body reads its element as one uncounted word. An element held
//! that way at the call site — a list, a string, a tuple holding a string —
//! shares a method the runtime settles; an element held as its bytes, such as
//! a tuple of scalars, does not.

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

/// A program that clears a list built from `element`.
fn clearing(element: &str) -> String {
    format!(
        r#"
use system.io
use system.collections.list

fn main()
    var xs = List([{element}])
    xs.insert(0, {element})
    xs.clear()
    println(f"{{xs.length()}}")
"#
    )
}

/// The `clear` bodies lowered for `element`'s list: whether the shared one
/// is, and the ones of its own.
fn clear_bodies(element: &str) -> (bool, Vec<String>) {
    let names = body_names(&clearing(element));
    let is_shared = names.iter().any(|name| name == "miri.List.clear");
    let own = names
        .into_iter()
        .filter(|name| name.starts_with("miri.List$") && name.ends_with(".clear"))
        .collect();
    (is_shared, own)
}

#[test]
fn test_clear_on_a_list_of_int_lists_shares_the_body() {
    assert_eq!(clear_bodies("List([1, 2])"), (true, Vec::<String>::new()));
}

#[test]
fn test_clear_on_a_list_of_tuples_holding_a_string_shares_the_body() {
    assert_eq!(clear_bodies(r#"(1, "x")"#), (true, Vec::<String>::new()));
}

/// The list's own body is the only one named, so the shared one, which
/// nothing calls, is not compiled.
#[test]
fn test_clear_on_a_list_of_scalar_tuples_reaches_a_body_of_its_own() {
    let (is_shared, own) = clear_bodies("(1, 2)");
    assert_eq!(own.len(), 1, "{own:?}");
    assert!(!is_shared, "nothing names the shared clear");
}

#[test]
fn test_clearing_lists_of_lists_and_tuples_runs_clean() {
    for element in ["List([1, 2])", r#"(1, "x")"#, "(1, 2)"] {
        assert_runs_with_output(&clearing(element), "0");
    }
}

/// A program that pops one element off a list built from `element`.
fn popping(element: &str) -> String {
    format!(
        r#"
use system.io
use system.collections.list

class Node
    var v int

fn main()
    var xs = List([{element}])
    match xs.pop()
        Some(_): println("some")
        None: println("none")
"#
    )
}

/// The `pop` bodies lowered for `element`'s list besides the shared one.
fn own_pop_bodies(element: &str) -> Vec<String> {
    body_names(&popping(element))
        .into_iter()
        .filter(|name| name.starts_with("miri.List$") && name.ends_with(".pop"))
        .collect()
}

/// An optional of `int` and an optional of the open parameter are both a
/// word beside a tag, so a list of `int` pops through the shared body. An
/// optional of a reference is that reference instead, and a float travels in
/// another register: each pops through a body of its own.
#[test]
fn test_pop_on_an_int_list_shares_the_body_its_optional_result_is_held_like() {
    assert_eq!(own_pop_bodies("1, 2"), Vec::<String>::new());
    assert_eq!(own_pop_bodies("Node(v: 1)").len(), 1);
    assert_eq!(own_pop_bodies("1.5").len(), 1);
    for element in ["1, 2", "Node(v: 1)", "1.5"] {
        assert_runs_with_output(&popping(element), "some");
    }
}
