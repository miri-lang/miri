// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{miri_run, strip_ansi};

#[test]
fn example_01_binary_search() {
    assert_example_contains(
        include_str!("./01_binary_search.mi"),
        "Binary Search result for 70: 6",
    );
}

#[test]
fn example_02_bubble_sort() {
    assert_example_contains(
        include_str!("./02_bubble_sort.mi"),
        "Sorted array:   1 5 11 12 22 25 34 45 64 90",
    );
}

#[test]
fn example_03_merge_sort() {
    assert_example_contains(
        include_str!("./03_merge_sort.mi"),
        "Sorted list:   3 9 10 27 38 43 82",
    );
}

#[test]
fn example_04_quick_sort() {
    assert_example_contains(
        include_str!("./04_quick_sort.mi"),
        "Sorted list:   10 30 40 50 70 80 90",
    );
}

#[test]
fn example_05_fibonacci() {
    assert_example_contains(include_str!("./05_fibonacci.mi"), "Fibonacci(20) = 6765");
}

#[test]
fn example_06_matrix_multiplication() {
    assert_example_contains(
        include_str!("./06_matrix_multiplication.mi"),
        "30 24 18 \n84 69 54 \n138 114 90",
    );
}

#[test]
fn example_07_tree_traversal() {
    assert_example_contains(
        include_str!("./07_tree_traversal.mi"),
        "In-order DFS traversal: 4 2 5 1 3",
    );
}

#[test]
fn example_08_graph_bfs() {
    assert_example_contains(
        include_str!("./08_graph_bfs.mi"),
        "BFS from node 2: 2 0 3 1",
    );
}

#[test]
fn example_09_linked_list_reverse() {
    assert_example_contains(
        include_str!("./09_linked_list_reverse.mi"),
        "50 -> 40 -> 30 -> 20 -> 10 -> None",
    );
}

#[test]
fn example_10_dijkstra() {
    assert_example_contains(
        include_str!("./10_dijkstra.mi"),
        "Node 0: 0\nNode 1: 3\nNode 2: 1\nNode 3: 4",
    );
}

#[test]
fn example_11_shape_hierarchy() {
    assert_example_contains(
        include_str!("./11_shape_hierarchy.mi"),
        "Rectangle (red): area = 24
Square (blue): area = 25
Triangle (green): area = 24
Colors: red, blue, green
Total area: 73
Largest area: 25
Doubled areas: 48 50 48
Rect perimeter: 20
Square perimeter: 20",
    );
}

#[test]
fn example_12_functional_pipeline() {
    assert_example_contains(
        include_str!("./12_functional_pipeline.mi"),
        "double(7)  = 14
square(6)  = 36
negate(5)  = -5
apply double to 9 = 18
apply inline       = 105
identity(42)  = 42
identity str  = pipeline
Doubled: 2 4 6 8 10
Squared: 1 4 9 16 25
Negated: -1 -2 -3 -4 -5
Sum 1..5     = 15
Product 1..5 = 120
Max 1..5     = 5
add_base(5)  = 105
add_base(20) = 120
Elements > 3: 2
Shifted+10: 11 12 13 14 15
x*3 then *2: 6 12 18 24 30
First > 3: 4
First > 10: -1",
    );
}

#[test]
fn example_13_employee_system() {
    assert_example_contains(
        include_str!("./13_employee_system.mi"),
        "Alice [Full-time] dept=Engineering salary=6000
Bob [Part-time] dept=Marketing salary=4000
Carol [Contractor] project=Phoenix salary=6000
Alice role:  Full-time
Bob role:    Part-time
Carol role:  Contractor
Alice dept:  Engineering
Bob dept:    Marketing
Carol dept:  external
Total payroll: 16000
Alice high earner: true
Bob high earner:   false
Carol high earner: true
High earners: 2
Payroll after 10% raise: 17600
Alice after raise: 6600
Bob after raise:   4400
Carol after raise: 6600
",
    )
}

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
