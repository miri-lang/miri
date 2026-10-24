// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method that writes its receiver, called on a collection another binding
//! shares, writes a copy only the receiver's binding holds: the other binding
//! keeps what it had, and the write is not lost. Methods that write through
//! other methods of their own (`resize` through `reserve`, `push` and `pop`)
//! count as writers too, as do the methods writing a collection held in a
//! field of a class.

use super::super::utils::*;

/// The program that binds `a` to `value`, shares it with `b`, calls `call` on
/// `b` and prints `report`, importing `module`.
fn shared_call(module: &str, value: &str, call: &str, report: &str) -> String {
    format!(
        "use system.io\nuse system.collections.{module}\n\nfn main()\n    var a = {value}\n    var b = a\n    b.{call}\n    println(f\"{report}\")\n"
    )
}

/// `call` on a list sharing `List([3, 1, 2])` prints `expected` for
/// `a`'s length and first element, then `b`'s.
fn assert_list_call(call: &str, expected: &str) {
    let report = "{a.length()} {a[0]} {b.length()} {b.length() > 0 and b[0] == 3}";
    assert_runs_with_output(
        &shared_call("list", "List([3, 1, 2])", call, report),
        expected,
    );
}

#[test]
fn test_resize_growing_a_shared_list_grows_only_the_receiver() {
    let report = "a={a.length()} b={b.length()} b5={b[5]}";
    assert_runs_with_output(
        &shared_call("list", "List([1, 2, 3])", "resize(6, 9)", report),
        "a=3 b=6 b5=9",
    );
}

#[test]
fn test_resize_shrinking_a_shared_list_shrinks_only_the_receiver() {
    let report = "a={a.length()} b={b.length()}";
    assert_runs_with_output(
        &shared_call("list", "List([1, 2, 3])", "resize(2, 0)", report),
        "a=3 b=2",
    );
}

#[test]
fn test_reserve_on_a_shared_list_leaves_the_other_binding_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var a = List([1, 2, 3])
    var b = a
    b.reserve(100)
    b.push(4)
    a.push(5)
    println(f"a={a.length()} {a[3]} b={b.length()} {b[3]}")
"#,
        "a=4 5 b=4 4",
    );
}

#[test]
fn test_every_list_writer_on_a_shared_list_writes_only_the_receiver() {
    for (call, expected) in [
        ("push(4)", "3 3 4 true"),
        ("set(0, 7)", "3 3 3 false"),
        ("insert(0, 7)", "3 3 4 false"),
        ("pop()", "3 3 2 true"),
        ("remove_at(0)", "3 3 2 false"),
        ("remove(3)", "3 3 2 false"),
        ("clear()", "3 3 0 false"),
        ("sort()", "3 3 3 false"),
        ("reverse()", "3 3 3 false"),
        ("reserve(10)", "3 3 3 true"),
        ("resize(1, 0)", "3 3 1 true"),
    ] {
        assert_list_call(call, expected);
    }
}

#[test]
fn test_every_map_writer_on_a_shared_map_writes_only_the_receiver() {
    let report = "{a.length()} {a.get(1) ?? -1} {b.length()} {b.get(1) ?? -1}";
    for (call, expected) in [
        ("set(1, 99)", "2 10 2 99"),
        ("remove(1)", "2 10 1 -1"),
        ("clear()", "2 10 0 -1"),
    ] {
        assert_runs_with_output(
            &shared_call("map", "{1: 10, 2: 20}", call, report),
            expected,
        );
    }
}

#[test]
fn test_every_set_writer_on_a_shared_set_writes_only_the_receiver() {
    let report = "{a.length()} {a.contains(1)} {b.length()} {b.contains(1)}";
    for (call, expected) in [
        ("add(9)", "2 true 3 true"),
        ("remove(1)", "2 true 1 false"),
        ("clear()", "2 true 0 false"),
    ] {
        assert_runs_with_output(&shared_call("set", "{1, 2}", call, report), expected);
    }
}

#[test]
fn test_every_array_writer_on_a_shared_array_writes_only_the_receiver() {
    let report = "{a[0]} {a[2]} {b[0]} {b[2]}";
    for (call, expected) in [
        ("set(0, 7)", "3 2 7 2"),
        ("sort()", "3 2 1 3"),
        ("reverse()", "3 2 2 3"),
    ] {
        assert_runs_with_output(&shared_call("array", "[3, 1, 2]", call, report), expected);
    }
}

#[test]
fn test_a_class_method_resizing_a_list_field_leaves_the_list_it_was_given() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Holder
    var items [int]

    fn init(items [int])
        self.items = items

    fn grow()
        self.items.resize(5, 0)

fn main()
    let source = List([1, 2])
    let holder = Holder(source)
    holder.grow()
    println(f"source={source.length()} items={holder.items.length()}")
"#,
        "source=2 items=5",
    );
}
