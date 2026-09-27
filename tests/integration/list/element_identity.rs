// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which element a list's `remove` treats as the one it was given.
//!
//! `remove` looks the element up by `==`: a string by its content, a class
//! that defines `equals` through that method. Every element compared here is
//! built at run time — two equal string literals share one pooled allocation,
//! so a literal would match by address and hide a list that never looks past
//! the pointer. The methods that pair an element read with a runtime take
//! (`pop`, `remove_at`) sit beside it so a change to which body `remove`
//! runs cannot unbalance them.

use super::utils::assert_runs_with_output;

#[test]
fn list_remove_finds_a_string_built_at_run_time_by_its_content() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var a = List(["p" + "", "q" + ""])
    let probe = "q"
    println(f"{a.index_of(probe) ?? -1}")
    let r = a.remove("q")
    println(f"{r} {a.length()}")
"#,
        "1\ntrue 1",
    );
}

#[test]
fn list_remove_matches_a_class_element_through_its_equals() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Tag
    name String

    fn init(name String)
        self.name = name

    public fn equals(other Tag) bool
        return self.name == other.name

fn main()
    var tags = List<Tag>()
    tags.push(Tag("A".to_lower()))
    tags.push(Tag("B".to_lower()))
    let removed = tags.remove(Tag("B".to_lower()))
    println(f"{removed} {tags.length()}")
"#,
        "true 1",
    );
}

#[test]
fn list_remove_of_an_absent_run_time_string_keeps_every_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var a = List(["p" + "", "q" + ""])
    let r = a.remove("Z".to_lower())
    println(f"{r} {a.length()}")
"#,
        "false 2",
    );
}

#[test]
fn list_remove_of_a_computed_string_releases_the_argument_and_the_list() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn shrink() int
    var a = List(["p" + "", "q" + "", "r" + ""])
    let hit = a.remove("Q".to_lower())
    let miss = a.remove("Z".to_lower())
    if hit and not miss: return a.length()
    return -1

fn main()
    println(f"{shrink()}")
"#,
        "2",
    );
}

#[test]
fn list_pop_and_remove_at_hand_out_run_time_strings_balanced() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var a = List(["p" + "", "q" + "", "r" + ""])
    let last = a.pop() ?? "none"
    let first = a.remove_at(0) ?? "none"
    println(f"{last} {first} {a.length()}")
    println(f"{a[0]}")
"#,
        "r p 1\nq",
    );
}

#[test]
fn list_remove_takes_only_the_first_of_equal_run_time_strings() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var a = List(["p" + "", "q" + "", "r" + "", "q" + ""])
    let r = a.remove("Q".to_lower())
    let rest = a.index_of("Q".to_lower()) ?? -1
    println(f"{r} {a.length()} {rest} {a[1]}{a[2]}")
"#,
        "true 3 2 rq",
    );
}

#[test]
fn list_remove_from_an_empty_list_of_strings_finds_nothing() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var a = List<String>()
    let r = a.remove("Q".to_lower())
    println(f"{r} {a.length()}")
"#,
        "false 0",
    );
}

#[test]
fn list_remove_of_a_wide_integer_compares_the_whole_value() {
    // An `i128` is wider than the word the shared body passes an element in,
    // so `remove` runs a body compiled for it; the two values below differ
    // only in their high word.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var a = List<i128>()
    let low i128 = 5
    let word i128 = 4294967296
    let high i128 = word * word + 5
    a.push(high)
    a.push(low)
    let r = a.remove(low)
    let left = a[0] == high
    println(f"{r} {a.length()} {left}")
"#,
        "true 1 true",
    );
}

#[test]
fn array_reverse_of_run_time_strings_stays_balanced() {
    // `reverse` swaps elements with plain stores, so a managed element runs a
    // body compiled for it rather than the shared one.
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    var a = ["p" + "", "q" + "", "r" + ""]
    a.reverse()
    println(f"{a[0]}{a[1]}{a[2]}")
    var b = ["w" + "", "x" + "", "y" + "", "z" + ""]
    b.reverse()
    b.reverse()
    println(f"{b[0]}{b[1]}{b[2]}{b[3]}")
"#,
        "rqp\nwxyz",
    );
}
