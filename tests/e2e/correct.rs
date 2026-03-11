// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{miri_run, strip_ansi};

/// Run the given source, assert success, and check that stdout contains the expected marker.
fn assert_example_contains(source: &str, expected_marker: &str) {
    let result = miri_run(source);
    if !result.success {
        panic!(
            "Expected program to run successfully, but got errors:\n{}",
            result.output()
        );
    }
    let actual = strip_ansi(&result.stdout);
    assert!(
        actual.contains(expected_marker),
        "Program output did not contain expected marker.\nExpected marker: {expected_marker}\nActual:\n{actual}"
    );
}

#[test]
fn example_01_binary_search() {
    assert_example_contains(
        include_str!("../examples/correct/01_binary_search.mi"),
        "Binary Search result for 70: 6",
    );
}

#[test]
fn example_02_bubble_sort() {
    assert_example_contains(
        include_str!("../examples/correct/02_bubble_sort.mi"),
        "Sorted array:   1 5 11 12 22 25 34 45 64 90",
    );
}

#[test]
fn example_03_merge_sort() {
    assert_example_contains(
        include_str!("../examples/correct/03_merge_sort.mi"),
        "Sorted list:   3 9 10 27 38 43 82",
    );
}

#[test]
fn example_04_quick_sort() {
    assert_example_contains(
        include_str!("../examples/correct/04_quick_sort.mi"),
        "Sorted list:   10 30 40 50 70 80 90",
    );
}

#[test]
fn example_05_fibonacci() {
    assert_example_contains(
        include_str!("../examples/correct/05_fibonacci.mi"),
        "Fibonacci(20) = 6765",
    );
}

#[test]
fn example_06_matrix_multiplication() {
    assert_example_contains(
        include_str!("../examples/correct/06_matrix_multiplication.mi"),
        "30 24 18 \n84 69 54 \n138 114 90",
    );
}

#[test]
fn example_07_tree_traversal() {
    assert_example_contains(
        include_str!("../examples/correct/07_tree_traversal.mi"),
        "In-order DFS traversal: 4 2 5 1 3",
    );
}

#[test]
fn example_08_graph_bfs() {
    assert_example_contains(
        include_str!("../examples/correct/08_graph_bfs.mi"),
        "BFS from node 2: 2 0 3 1",
    );
}

#[test]
fn example_09_linked_list_reverse() {
    assert_example_contains(
        include_str!("../examples/correct/09_linked_list_reverse.mi"),
        "Reversed list: 5 -> 4 -> 3 -> 2 -> 1 -> None",
    );
}

#[test]
fn example_10_dijkstra() {
    assert_example_contains(
        include_str!("../examples/correct/10_dijkstra.mi"),
        "Node 0: 0\nNode 1: 4\nNode 2: 12\nNode 3: 19\nNode 7: 8",
    );
}
