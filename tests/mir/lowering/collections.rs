// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Integration tests for collection literal lowering and index access.

use super::utils::{lowering_test_aggregate, lowering_test_index_projection};
use miri::mir::AggregateKind;

// === Collection Literal Tests ===

#[test]
fn test_tuple_literal() {
    lowering_test_aggregate(
        "
fn main()
    let t = (1, 2, 3)
",
        AggregateKind::Tuple,
        3,
    );
}

#[test]
fn test_empty_tuple() {
    lowering_test_aggregate(
        "
fn main()
    let unit = ()
",
        AggregateKind::Tuple,
        0,
    );
}

#[test]
fn test_list_literal() {
    lowering_test_aggregate(
        "
fn main()
    let l = [1, 2, 3]
",
        AggregateKind::List,
        3,
    );
}

#[test]
fn test_set_literal() {
    lowering_test_aggregate(
        "
fn main()
    let s = {1, 2, 3}
",
        AggregateKind::Set,
        3,
    );
}

#[test]
fn test_map_literal() {
    // Map has key1, val1, key2, val2 = 4 operands for 2 pairs
    lowering_test_aggregate(
        "
fn main()
    let m = {\"a\": 1, \"b\": 2}
",
        AggregateKind::Map,
        4,
    );
}

#[test]
fn test_nested_tuple() {
    // Should find the outer tuple (at least 2 elements - the inner tuples)
    lowering_test_aggregate(
        "
fn main()
    let nested = ((1, 2), (3, 4))
",
        AggregateKind::Tuple,
        2,
    );
}

#[test]
fn test_list_index_access() {
    lowering_test_index_projection(
        "
fn main()
    let l = [10, 20, 30]
    let x = l[1]
",
    );
}

#[test]
fn test_tuple_index_access() {
    lowering_test_index_projection(
        "
fn main()
    let t = (1, 2, 3)
    let x = t[0]
",
    );
}

#[test]
fn test_map_index_access() {
    lowering_test_index_projection(
        "
fn main()
    let m = {\"a\": 1, \"b\": 2}
    let x = m[\"a\"]
",
    );
}

#[test]
fn test_computed_index() {
    lowering_test_index_projection(
        "
fn main()
    let l = [1, 2, 3]
    var i = 1
    let x = l[i]
",
    );
}

#[test]
fn test_empty_list() {
    lowering_test_aggregate(
        "
fn main()
    let l [int] = []
",
        AggregateKind::List,
        0,
    );
}

#[test]
fn test_nested_list() {
    lowering_test_aggregate(
        "
fn main()
    let l = [[1, 2], [3, 4]]
",
        AggregateKind::List,
        2,
    );
}

#[test]
fn test_single_element_tuple() {
    lowering_test_aggregate(
        "
fn main()
    let t = (42,)
",
        AggregateKind::Tuple,
        1,
    );
}
