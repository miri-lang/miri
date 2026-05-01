// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Phase 10: Copy-on-Write value semantics ───────────────────────────────────

#[test]
fn test_list_cow_push_isolates_original() {
    // The canonical CoW test from the spec.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.push(4)
    println(f"{a.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_cow_push_new_has_element() {
    // After CoW push, the mutated alias contains the new element.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.push(4)
    println(f"{b.length()}")
    println(f"{b[3]}")
"#,
        "4\n4",
    );
}

#[test]
fn test_list_cow_insert_isolates_original() {
    // insert triggers CoW — original must be unaffected.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.insert(0, 99)
    println(f"{a.length()}")
    println(f"{a[0]}")
"#,
        "3\n1",
    );
}

#[test]
fn test_list_cow_clear_isolates_original() {
    // clear triggers CoW — original must be unaffected.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.clear()
    println(f"{a.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_cow_set_isolates_original() {
    // set (indexed mutation) triggers CoW — original element unchanged.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.set(0, 99)
    println(f"{a[0]}")
"#,
        "1",
    );
}

#[test]
fn test_list_cow_remove_at_isolates_original() {
    // remove_at triggers CoW — original must retain all elements.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.remove_at(0)
    println(f"{a.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_cow_no_copy_when_rc_one() {
    // When only one owner, mutation must not allocate a new list.
    // Verify by observing the element is updated in place.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var a = List([1, 2, 3])
    a.push(4)
    println(f"{a.length()}")
    println(f"{a[3]}")
"#,
        "4\n4",
    );
}

#[test]
fn test_list_cow_triple_alias_isolates_all() {
    // Three aliases — each push triggers its own CoW.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    var c = a
    b.push(10)
    c.push(20)
    println(f"{a.length()}")
    println(f"{b[3]}")
    println(f"{c[3]}")
"#,
        "3\n10\n20",
    );
}

#[test]
fn test_list_cow_pop_isolates_original() {
    // pop triggers CoW — original must be unaffected.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    let _ = b.pop()
    println(f"{a.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_cow_sort_isolates_original() {
    // sort triggers CoW — original order must be unchanged.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([3, 1, 2])
    var b = a
    b.sort()
    println(f"{a[0]}")
"#,
        "3",
    );
}

#[test]
fn test_list_cow_remove_isolates_original() {
    // remove(item) triggers CoW — original must retain all elements.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a
    b.remove(2)
    println(f"{a.length()}")
    println(f"{a[1]}")
"#,
        "3\n2",
    );
}

#[test]
fn test_list_cow_no_leak_on_cow_and_free() {
    // CoW and then immediate scope exit must not leak.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn mutate(l [int]) [int]
    var m = l
    m.push(99)
    return m

fn main()
    let a = List([1, 2, 3])
    let b = mutate(a)
    println(f"{a.length()}")
    println(f"{b.length()}")
"#,
        "3\n4",
    );
}
