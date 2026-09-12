// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading a field off an indexed collection element.
//!
//! The element's own type decides the field's offset. Resolving the offset
//! against the collection instead reads a neighbouring slot, so these tests
//! pin the value each field carries rather than only that the program runs.

use super::utils::*;

#[test]
fn list_element_managed_field_reads_its_own_value() {
    assert_repeated_runs_have_output(
        "
use system.collections.list

struct Word
    text String
    count int

fn main()
    var words = List<Word>()
    words.push(Word(\"apple\", 5))
    let a = words[0].text
    println(a)
",
        "apple",
        50,
    );
}

#[test]
fn list_element_first_field_is_not_the_second() {
    assert_runs_with_output(
        "
use system.collections.list

struct Word
    count int
    text String

fn main()
    var words = List<Word>()
    words.push(Word(5, \"apple\"))
    println(f\"{words[0].count}\")
    println(words[0].text)
",
        "5\napple",
    );
}

#[test]
fn list_element_field_survives_as_a_call_argument() {
    assert_runs_with_output(
        "
use system.collections.list

struct Word
    text String
    count int

fn shout(word String) String
    return word.to_upper()

fn main()
    var words = List<Word>()
    words.push(Word(\"apple\", 5))
    println(shout(words[0].text))
",
        "APPLE",
    );
}

#[test]
fn list_element_field_renders_inside_a_formatted_string() {
    assert_runs_with_output(
        "
use system.collections.list

struct Word
    text String
    count int

fn main()
    var words = List<Word>()
    words.push(Word(\"apple\", 5))
    words.push(Word(\"pear\", 2))
    println(f\"{words[0].text}={words[0].count}\")
    println(f\"{words[1].text}={words[1].count}\")
",
        "apple=5\npear=2",
    );
}

#[test]
fn list_element_field_drives_a_while_condition() {
    assert_runs_with_output(
        "
use system.collections.list

struct Word
    count int
    text String

fn main()
    var words = List<Word>()
    words.push(Word(3, \"apple\"))
    var seen = 0
    while seen < words[0].count
        seen = seen + 1
    println(f\"{seen}\")
",
        "3",
    );
}

#[test]
fn array_element_managed_field_reads_its_own_value() {
    assert_runs_with_output(
        "
struct Word
    text String
    count int

fn main()
    let words = [Word(\"apple\", 5), Word(\"pear\", 2)]
    println(words[0].text)
    println(f\"{words[0].count}\")
",
        "apple\n5",
    );
}

#[test]
fn class_element_field_reads_the_same_bound_or_inline() {
    assert_runs_with_output(
        "
use system.collections.list

class WordCount
    public var count int

    public fn init(count int)
        self.count = count

fn rows() [WordCount]
    return List([WordCount(3), WordCount(4)])

fn main()
    let r = rows()
    println(f\"{r[0].count}\")
    let first = r[0]
    println(f\"{first.count}\")
",
        "3\n3",
    );
}

#[test]
fn nested_field_off_a_list_element_reads_its_own_value() {
    assert_runs_with_output(
        "
use system.collections.list

struct Inner
    tag String
    n int

struct Outer
    inner Inner
    id int

fn main()
    var rows = List<Outer>()
    rows.push(Outer(Inner(\"deep\", 7), 1))
    println(rows[0].inner.tag)
    println(f\"{rows[0].inner.n}\")
    println(f\"{rows[0].id}\")
",
        "deep\n7\n1",
    );
}

#[test]
fn tuple_element_field_reads_its_own_value() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    var rows = List<(String, int)>()
    rows.push((\"a\", 1))
    rows.push((\"b\", 2))
    println(f\"{rows[1].0}:{rows[1].1}\")
",
        "b:2",
    );
}

#[test]
fn list_element_managed_field_read_leaves_the_heap_clean() {
    assert_heap_guard_ok(
        "
use system.collections.list

struct Word
    text String
    count int

fn main()
    var words = List<Word>()
    words.push(Word(\"apple\", 5))
    let a = words[0].text
    println(a)
",
    );
}
