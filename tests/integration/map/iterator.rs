// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A map's iterator: `iterator()` hands out a `MapIterator`, whose `next()`
//! yields every key once and `None` from then on. A map's order is
//! unspecified, so these tests assert sums, counts and sorted output.
//!
//! `iterator()` is declared to return `Iterator<K>`, so `value()` is reached
//! through a `MapIterator` built directly.

use super::utils::*;

#[test]
fn map_iterator_yields_every_string_key_once() {
    assert_runs_with_output(
        r#"
use system.collections.map
use system.collections.list

fn main()
    var m = Map<String, int>()
    m.set("pe" + "ar", 1)
    m.set("fig", 2)
    m.set("ap" + "ple", 3)
    let it = m.iterator()
    var lines = List<String>()
    var walking = true
    while walking
        if let Some(key) = it.next()
            let value = m.get(key) ?? 0
            lines.push(f"{key} = {value}")
        else
            walking = false
    lines.sort()
    for line in lines
        println(line)
"#,
        "apple = 3\nfig = 2\npear = 1\n",
    );
}

#[test]
fn map_iterator_over_an_empty_map_yields_nothing() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let m = Map<String, int>()
    let it = m.iterator()
    match it.next()
        Some(key): println(f"key {key}")
        None: println("none")
"#,
        "none",
    );
}

#[test]
fn map_iterator_over_one_entry_yields_its_key_then_none() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let m = {"only": 5}
    let it = m.iterator()
    let first = it.next() ?? "none"
    let second = it.next() ?? "none"
    let third = it.next() ?? "none"
    println(f"{first} {second} {third}")
"#,
        "only none none",
    );
}

#[test]
fn map_iterator_walks_the_map_as_it_was_when_written_to_during_the_walk() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    m.set("a" + "1", 1)
    m.set("a" + "2", 2)
    m.set("a" + "3", 3)
    let it = m.iterator()
    var count = 0
    var walking = true
    while walking
        if let Some(key) = it.next()
            count += 1
            m.set(key + "!", 10)
            m.remove("a1")
        else
            walking = false
    let removed = m.contains_key("a1")
    let added = m.contains_key("a3!")
    println(f"{count} {m.length()} {removed} {added}")
"#,
        "3 5 false true",
    );
}

#[test]
fn map_iterator_yields_every_int_key_once() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let m = {1: 10, 2: 20, 3: 30}
    let it = m.iterator()
    var key_sum = 0
    var value_sum = 0
    var walking = true
    while walking
        if let Some(key) = it.next()
            key_sum += key
            value_sum += m.get(key) ?? 0
        else
            walking = false
    println(f"{key_sum} {value_sum}")
"#,
        "6 60",
    );
}

#[test]
fn map_iterator_value_is_the_value_of_the_key_next_returned() {
    assert_heap_guard_output(
        r#"
use system.collections.map
use system.collections.list

fn main()
    var m = Map<String, String>()
    m.set("pe" + "ar", "green")
    m.set("fig", "pur" + "ple")
    m.set("ap" + "ple", "red")
    let it = MapIterator<String, String>(source: m)
    var lines = List<String>()
    var walking = true
    while walking
        if let Some(key) = it.next()
            let value = it.value() ?? "none"
            lines.push(f"{key} = {value}")
        else
            walking = false
    lines.sort()
    for line in lines
        println(line)
"#,
        "apple = red\nfig = purple\npear = green",
    );
}

#[test]
fn map_iterator_value_is_none_before_the_first_next() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    let m = {"only": 5}
    let it = MapIterator<String, int>(source: m)
    match it.value()
        Some(value): println(f"value {value}")
        None: println("none")
"#,
        "none",
    );
}

#[test]
fn map_iterator_value_is_none_once_exhausted() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    let m = {"only": 5}
    let it = MapIterator<String, int>(source: m)
    let first = it.next() ?? "none"
    let after_first = it.value() ?? -1
    let second = it.next() ?? "none"
    let after_second = it.value() ?? -1
    println(f"{first} {after_first} {second} {after_second}")
"#,
        "only 5 none -1",
    );
}

#[test]
fn map_iterator_value_reads_the_map_as_it_was_when_written_to_during_the_walk() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = {1: 10, 2: 20, 3: 30}
    let it = MapIterator<int, int>(source: m)
    var sum = 0
    var walking = true
    while walking
        if let Some(key) = it.next()
            m.set(key, 0)
            sum += it.value() ?? 0
        else
            walking = false
    println(f"{sum} {m.get(1) ?? -1}")
"#,
        "60 0",
    );
}
