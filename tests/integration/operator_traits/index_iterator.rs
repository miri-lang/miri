// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `Indexed<T>` walks anything with `length()` and `element_at()` by
//! position: its default `iterator()` hands out an `IndexIterator`, whose
//! `next()` yields each element in order and whose `value()` is the index of
//! the element `next()` last returned.
//!
//! `iterator()` is declared to return `Iterator<T>`, so `value()` is reached
//! through an `IndexIterator` built directly.

use super::utils::*;

#[test]
fn index_iterator_yields_a_lists_elements_in_order() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let words = List(["zero", "one", "two"])
    let it = words.iterator()
    var walking = true
    while walking
        if let Some(word) = it.next()
            println(word)
        else
            walking = false
"#,
        "zero\none\ntwo",
    );
}

#[test]
fn index_iterator_value_is_the_index_of_the_element_next_returned() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let words = List(["zero", "one", "two"])
    let it = IndexIterator<String, List<String>>(source: words)
    var walking = true
    while walking
        if let Some(word) = it.next()
            let index = it.value() ?? -1
            println(f"{index} {word}")
        else
            walking = false
"#,
        "0 zero\n1 one\n2 two",
    );
}

#[test]
fn index_iterator_value_is_none_before_the_first_next() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let numbers = List([7, 8])
    let it = IndexIterator<int, List<int>>(source: numbers)
    match it.value()
        Some(index): println(f"index {index}")
        None: println("none")
"#,
        "none",
    );
}

#[test]
fn index_iterator_value_is_none_once_exhausted() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let words = List(["only"])
    let it = IndexIterator<String, List<String>>(source: words)
    let first = it.next() ?? "none"
    let after_first = it.value() ?? -1
    let second = it.next() ?? "none"
    let after_second = it.value() ?? -1
    let third = it.next() ?? "none"
    println(f"{first} {after_first} {second} {after_second} {third}")
"#,
        "only 0 none -1 none",
    );
}

#[test]
fn index_iterator_over_an_empty_list_yields_nothing() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let words = List<String>()
    let it = words.iterator()
    let first = it.next() ?? "none"
    println(first)
"#,
        "none",
    );
}

#[test]
fn index_iterator_walks_the_list_as_it_was_when_written_to_during_the_walk() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var words = List(["a", "b"])
    let it = words.iterator()
    var seen = List<String>()
    var walking = true
    while walking
        if let Some(word) = it.next()
            seen.push(word)
            words.push(word + "!")
        else
            walking = false
    println(f"{seen.length()} {words.length()} {seen[1]} {words[3]}")
"#,
        "2 4 b b!",
    );
}

#[test]
fn index_iterator_walks_an_array() {
    assert_heap_guard_output(
        r#"
use system.collections.array

fn main()
    let numbers = [3, 4, 5]
    let it = numbers.iterator()
    var sum = 0
    var walking = true
    while walking
        if let Some(n) = it.next()
            sum += n
        else
            walking = false
    println(f"{sum}")
"#,
        "12",
    );
}

#[test]
fn index_iterator_walks_a_strings_characters() {
    assert_heap_guard_output(
        r#"
fn main()
    let word = "h" + "ey"
    let it = word.iterator()
    var walking = true
    while walking
        if let Some(c) = it.next()
            println(c)
        else
            walking = false
"#,
        "h\ne\ny",
    );
}

#[test]
fn user_indexed_class_is_walked_through_the_default_iterator() {
    assert_heap_guard_output(
        r#"
use system.ops

class Squares implements Indexed<int>
    var count int
    fn init(count int)
        self.count = count
    public fn length() int: self.count
    public fn element_at(index int) int: index * index

fn main()
    let squares = Squares(count: 4)
    let it = squares.iterator()
    var walking = true
    while walking
        if let Some(n) = it.next()
            println(f"{n}")
        else
            walking = false
"#,
        "0\n1\n4\n9",
    );
}

#[test]
fn indexed_class_without_element_at_is_refused() {
    assert_compiler_error(
        r#"
use system.ops

class Broken implements Indexed<int>
    public fn length() int: 0

fn main()
    let b = Broken()
    println(f"{b.length()}")
"#,
        "element_at",
    );
}

#[test]
fn index_iterator_walks_a_list_of_ints() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let numbers = List([3, 4, 5])
    let it = numbers.iterator()
    var sum = 0
    var walking = true
    while walking
        if let Some(n) = it.next()
            sum += n
        else
            walking = false
    println(f"{sum}")
"#,
        "12",
    );
}

#[test]
fn index_iterator_walks_a_list_of_bools() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let flags = List([true, false, true])
    let it = flags.iterator()
    var walking = true
    while walking
        if let Some(flag) = it.next()
            println(f"{flag}")
        else
            walking = false
"#,
        "true\nfalse\ntrue",
    );
}
