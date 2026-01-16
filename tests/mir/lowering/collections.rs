// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lowering_aggregate_test, mir_lowering_index_test};
use miri::mir::AggregateKind;

#[test]
fn test_tuple_literal() {
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_index_test(
        "
fn main()
    let l = [10, 20, 30]
    let x = l[1]
",
    );
}

#[test]
fn test_tuple_index_access() {
    mir_lowering_index_test(
        "
fn main()
    let t = (1, 2, 3)
    let x = t[0]
",
    );
}

#[test]
fn test_map_index_access() {
    mir_lowering_index_test(
        "
fn main()
    let m = {\"a\": 1, \"b\": 2}
    let x = m[\"a\"]
",
    );
}

#[test]
fn test_computed_index() {
    mir_lowering_index_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
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
    mir_lowering_aggregate_test(
        "
fn main()
    let t = (42,)
",
        AggregateKind::Tuple,
        1,
    );
}

#[test]
fn test_large_tuple() {
    mir_lowering_aggregate_test(
        "
fn main()
    let t = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
",
        AggregateKind::Tuple,
        10,
    );
}

#[test]
fn test_large_list() {
    mir_lowering_aggregate_test(
        "
fn main()
    let l = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
",
        AggregateKind::List,
        15,
    );
}

#[test]
fn test_deeply_nested_lists() {
    mir_lowering_aggregate_test(
        "
fn main()
    let l = [[[1, 2], [3, 4]], [[5, 6], [7, 8]]]
",
        AggregateKind::List,
        2,
    );
}

#[test]
fn test_mixed_nested_collections() {
    mir_lowering_aggregate_test(
        "
fn main()
    let t = ([1, 2], [3, 4])
",
        AggregateKind::Tuple,
        2,
    );
}

#[test]
fn test_tuple_of_tuples() {
    mir_lowering_aggregate_test(
        "
fn main()
    let inner1 = (1, 2)
    let inner2 = (3, 4)
    let inner3 = (5, 6)
    let t = (inner1, inner2, inner3)
",
        AggregateKind::Tuple,
        2,
    );
}

#[test]
fn test_map_with_many_entries() {
    mir_lowering_aggregate_test(
        "
fn main()
    let m = {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4, \"e\": 5}
",
        AggregateKind::Map,
        10,
    );
}

#[test]
fn test_set_with_many_elements() {
    mir_lowering_aggregate_test(
        "
fn main()
    let s = {1, 2, 3, 4, 5, 6, 7, 8}
",
        AggregateKind::Set,
        8,
    );
}

#[test]
fn test_index_expression_in_index() {
    mir_lowering_index_test(
        "
fn main()
    let l = [[1, 2], [3, 4]]
    let x = l[0][1]
",
    );
}

#[test]
fn test_index_last_element() {
    mir_lowering_index_test(
        "
fn main()
    let l = [10, 20, 30, 40, 50]
    let x = l[4]
",
    );
}
