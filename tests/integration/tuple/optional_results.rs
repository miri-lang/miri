// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The tuple methods that hand an element back as an optional: `first` and
//! `last`, which a tuple gets from `Queryable`.
//!
//! An optional of a string or a class is laid out differently from one boxing
//! a scalar, so a tuple of such elements reaches a body compiled for its
//! element type rather than the one shared by every tuple. The value tests run
//! those bodies under the leak check and the MIR verifier.

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

const STRING_ENDS: &str = r#"
use system.io
use system.collections.tuple

fn main()
    let t = ("hello", "world", "foo")
    let first = t.first() ?? "none"
    let last = t.last() ?? "none"
    println(f"{first} {last}")
"#;

#[test]
fn test_first_and_last_on_a_string_tuple_reach_bodies_of_their_own() {
    let names = body_names(STRING_ENDS);
    for suffix in ["Tuple$String.first", "Tuple$String.last"] {
        assert!(
            names.iter().any(|name| name.ends_with(suffix)),
            "no body named *{suffix} among {names:?}"
        );
    }
}

#[test]
fn test_first_and_last_hand_back_string_elements() {
    assert_runs_with_output(STRING_ENDS, "hello foo");
}

#[test]
fn test_first_and_last_hand_back_class_elements() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

class Node
    var value int

fn value_of(node Node?) int
    match node
        Some(n): return n.value
        None: return -1

fn main()
    let t = (Node(value: 1), Node(value: 2), Node(value: 3))
    println(f"{value_of(t.first())} {value_of(t.last())}")
"#,
        "1 3",
    );
}
