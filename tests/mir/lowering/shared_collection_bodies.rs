// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which built-in collection instances share the body compiled once with the
//! collection's parameters open.
//!
//! An instance shares it only if every value a caller can reach through a
//! method's signature is represented alike: a returned trait value whose own
//! methods hand out an optional of the element is not, at an element type the
//! shared body holds differently; a returned collection is judged by this same
//! rule at each of its own calls, so it stays shared.

use miri::pipeline::Pipeline;

/// The names of every body the program lowers.
fn lowered_body_names(source: &str) -> Vec<String> {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("program should lower")
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Whether some lowered body's name ends with `suffix`.
fn has_body_ending(names: &[String], suffix: &str) -> bool {
    names.iter().any(|name| name.ends_with(suffix))
}

#[test]
fn a_returned_trait_handing_out_an_optional_element_is_not_shared_at_int() {
    let names = lowered_body_names(
        "
use system.collections.list

fn main()
    let numbers = List([1, 2])
    let it = numbers.iterator()
    let first = it.next() ?? -1
",
    );
    assert!(
        has_body_ending(&names, "List$int.iterator"),
        "List<int>.iterator should be compiled for the instance: {names:?}"
    );
    assert!(
        !has_body_ending(&names, "List.iterator"),
        "the shared List.iterator should not be reached: {names:?}"
    );
}

#[test]
fn a_returned_collection_and_an_optional_element_stay_shared_at_int() {
    let names = lowered_body_names(
        "
use system.collections.list

fn main()
    let numbers = List([1, 2])
    let first = numbers.first() ?? -1
    let kept = numbers.filter(fn(n int) bool: n > 0)
",
    );
    assert!(
        has_body_ending(&names, "List.first") && has_body_ending(&names, "List.filter"),
        "List<int>.first and .filter should share the open bodies: {names:?}"
    );
    assert!(
        !has_body_ending(&names, "List$int.first") && !has_body_ending(&names, "List$int.filter"),
        "no per-instance copy of first or filter should be compiled: {names:?}"
    );
}
