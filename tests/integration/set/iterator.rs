// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A set's iterator: `iterator()` hands out a `SetIterator`, whose `next()`
//! yields every element once and `None` from then on. A set's order is
//! unspecified, so these tests assert sums, counts and sorted output.

use super::utils::*;

#[test]
fn set_iterator_yields_every_int_once() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let s = {10, 20, 30, 40}
    let it = s.iterator()
    var sum = 0
    var count = 0
    var walking = true
    while walking
        match it.next()
            Some(x)
                sum += x
                count += 1
            None: walking = false
    println(f"{sum} {count}")
"#,
        "100 4",
    );
}

#[test]
fn set_iterator_yields_every_string_once() {
    assert_runs_with_output(
        r#"
use system.collections.set
use system.collections.list

fn main()
    var s = Set<String>()
    s.add("pe" + "ar")
    s.add("fig")
    s.add("ap" + "ple")
    let it = s.iterator()
    var seen = List<String>()
    var walking = true
    while walking
        if let Some(word) = it.next()
            seen.push(word)
        else
            walking = false
    seen.sort()
    for word in seen
        println(word)
"#,
        "apple\nfig\npear\n",
    );
}

#[test]
fn set_iterator_over_an_empty_set_yields_nothing() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let s = Set<String>()
    let it = s.iterator()
    match it.next()
        Some(x): println(f"element {x}")
        None: println("none")
"#,
        "none",
    );
}

#[test]
fn set_iterator_over_one_element_yields_it_then_none() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let s = {7}
    let it = s.iterator()
    let first = it.next() ?? -1
    let second = it.next() ?? -1
    println(f"{first} {second}")
"#,
        "7 -1",
    );
}

#[test]
fn set_iterator_keeps_answering_none_once_exhausted() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("a" + "b")
    s.add("c" + "d")
    let it = s.iterator()
    var count = 0
    var asked = 0
    while asked < 5
        if let Some(_) = it.next()
            count += 1
        asked += 1
    let after = it.next() ?? "none"
    println(f"{count} {after}")
"#,
        "2 none",
    );
}

#[test]
fn set_iterator_walks_the_set_as_it_was_when_written_to_during_the_walk() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = {1, 2, 3}
    let it = s.iterator()
    var sum = 0
    var count = 0
    var walking = true
    while walking
        match it.next()
            Some(x)
                sum += x
                count += 1
                s.add(x * 100)
                s.remove(1)
            None: walking = false
    println(f"{count} {sum} {s.length()} {s.contains(1)} {s.contains(300)}")
"#,
        "3 6 5 false true",
    );
}

#[test]
fn set_iterator_of_strings_walks_the_original_when_written_to_during_the_walk() {
    assert_runs_with_output(
        r#"
use system.collections.set
use system.collections.list

fn main()
    var s = Set<String>()
    s.add("x" + "1")
    s.add("x" + "2")
    let it = s.iterator()
    var seen = List<String>()
    var walking = true
    while walking
        if let Some(word) = it.next()
            seen.push(word)
            s.add(word + "!")
        else
            walking = false
    let grown = s.contains("x1!")
    println(f"{seen.length()} {s.length()} {grown}")
"#,
        "2 4 true",
    );
}
