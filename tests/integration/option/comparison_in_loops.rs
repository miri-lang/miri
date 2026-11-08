// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Comparing an optional class inside a loop. The comparison reads each
// side's payload into a temporary; that temporary must be released when the
// comparison is done, not when the enclosing scope ends, or every turn of the
// loop leaves one reference behind.

use super::utils::*;

const NODE: &str = r#"
class Node
    var val int
    var next Node?
    fn init(v int)
        self.val = v
    fn drop(self)
        println(f"drop {self.val}")
"#;

fn with_node(program: &str) -> String {
    format!("{NODE}{program}")
}

#[test]
fn test_option_compare_none_in_while_with_if_let_walk() {
    assert_heap_guard_output(
        &with_node(
            r#"
fn walk(head Node?) int
    var c = 0
    var cur = head
    while cur != None
        c += 1
        if let Some(nd) = cur
            cur = nd.next
    return c

fn main()
    var a = Node(1)
    var b = Node(2)
    a.next = b
    println(f"{walk(a)}")
    println("end")
"#,
        ),
        "2\nend\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_option_compare_none_in_while_with_match_walk() {
    assert_heap_guard_output(
        &with_node(
            r#"
fn walk(head Node?) int
    var c = 0
    var cur = head
    while cur != None
        c += 1
        match cur
            Some(nd): cur = nd.next
            None: c += 0
    return c

fn main()
    var a = Node(1)
    var b = Node(2)
    a.next = b
    println(f"{walk(a)}")
    println("end")
"#,
        ),
        "2\nend\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_option_compare_some_in_while_releases_payloads() {
    assert_heap_guard_output(
        &with_node(
            r#"
fn walk(head Node?, stop Node?) int
    var c = 0
    var cur = head
    while cur != stop
        c += 1
        if let Some(nd) = cur
            cur = nd.next
    return c

fn main()
    var a = Node(1)
    var b = Node(2)
    var d = Node(3)
    b.next = d
    a.next = b
    println(f"{walk(a, d)}")
    println("end")
"#,
        ),
        "2\nend\ndrop 1\ndrop 2\ndrop 3",
    );
}

#[test]
fn test_option_compare_walk_returns_early() {
    assert_heap_guard_output(
        r#"
class Cell
    var val int
    var next Cell?
    fn init(v int)
        self.val = v

fn find(head Cell?, wanted int) int
    var cur = head
    while cur != None
        if let Some(cell) = cur
            if cell.val == wanted
                return cell.val
            cur = cell.next
    return -1

fn main()
    var a = Cell(1)
    var b = Cell(2)
    var d = Cell(3)
    b.next = d
    a.next = b
    println(f"{find(a, 2)}")
    println(f"{find(a, 9)}")
    println("end")
"#,
        "2\n-1\nend",
    );
}

#[test]
fn test_option_compare_walks_thousand_node_list() {
    assert_heap_guard_output(
        r#"
class Cell
    var val int
    var next Cell?
    fn init(v int)
        self.val = v

fn build(n int) Cell?
    var head Cell? = None
    for i in 0..n
        var cell = Cell(i)
        cell.next = head
        head = cell
    return head

fn walk(head Cell?) int
    var total = 0
    var cur = head
    while cur != None
        match cur
            Some(cell)
                total += cell.val
                cur = cell.next
            None: total += 0
    return total

fn main()
    let list = build(1000)
    println(f"{walk(list)}")
    println(f"{walk(list)}")
"#,
        "499500\n499500",
    );
}
