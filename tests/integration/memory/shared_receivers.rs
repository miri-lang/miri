// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method that writes its receiver, called on a collection another binding
//! shares, writes a copy only the receiver's binding holds: the other binding
//! keeps what it had, and the write is not lost. Methods that write through
//! other methods of their own (`resize` through `reserve`, `push` and `pop`)
//! count as writers too, as do the methods writing a collection held in a
//! field of a class.
//!
//! A receiver no binding holds — a call's result, a getter's, a conditional's,
//! or a field of one — is a value of its own as well: a write into it never
//! reaches the collection it was read from, and the temporary is released once
//! the write is done.

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

/// The program that declares `decls`, then runs `body` inside `main`,
/// importing the list module.
fn list_program(decls: &str, body: &str) -> String {
    format!("use system.io\nuse system.collections.list\n\n{decls}\nfn main()\n{body}")
}

/// `fn mk(a [int]) [int]: a`, the call whose result shares its argument.
const MK: &str = "fn mk(a [int]) [int]: a\n";

#[test]
fn test_every_list_writer_on_a_call_result_leaves_the_argument_alone() {
    for call in [
        "resize(6, 9)",
        "push(7)",
        "pop()",
        "clear()",
        "sort()",
        "set(0, 7)",
        "insert(0, 7)",
    ] {
        assert_runs_with_output(
            &list_program(
                MK,
                &format!(
                    "    var a = List([3, 2, 1])\n    mk(a).{call}\n    println(f\"{{a.length()}} {{a[0]}}\")\n"
                ),
            ),
            "3 3",
        );
    }
}

#[test]
fn test_map_and_set_writers_on_a_call_result_leave_the_argument_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

fn mm(m Map<String, int>) Map<String, int>: m
fn ms(s Set<int>) Set<int>: s

fn main()
    var m = Map<String, int>()
    m.set("a", 1)
    mm(m).set("b", 2)
    mm(m).remove("a")
    var s = Set<int>()
    s.add(1)
    ms(s).add(2)
    ms(s).clear()
    println(f"{m.length()} {s.length()}")
"#,
        "1 1",
    );
}

#[test]
fn test_a_writer_on_a_fresh_call_result_copies_nothing() {
    let line = super::rc_stats::rc_stats(
        &list_program(
            "fn fresh() [int]: List([1, 2, 3])\n",
            "    fresh().push(1)\n    fresh().resize(6, 9)\n    fresh().set(0, 4)\n    println(\"ok\")\n",
        ),
        &[],
    );
    assert!(line.contains("cow_copy=0"), "{line}");
}

#[test]
fn test_a_writer_on_a_getter_result_leaves_the_field_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Box
    public var items [int]
    public fn init()
        self.items = List([1, 2, 3])
    public fn get() [int]: self.items

fn main()
    let bx = Box()
    var keep = bx.items
    bx.get().resize(6, 9)
    bx.get().push(4)
    bx.get().clear()
    println(f"{keep.length()} {bx.items.length()}")
"#,
        "3 3",
    );
}

#[test]
fn test_a_writer_on_a_field_of_a_call_result_leaves_the_struct_alone() {
    assert_runs_with_output(
        &list_program(
            "struct S\n    items [int]\n\nfn mk(s S) S: s\n",
            "    var a = List([1, 2, 3])\n    var s = S(items: a)\n    mk(s).items.resize(6, 9)\n    mk(s).items.push(1)\n    println(f\"{a.length()} {s.items.length()}\")\n",
        ),
        "3 3",
    );
}

#[test]
fn test_a_writer_on_an_element_of_a_returned_tuple_leaves_the_argument_alone() {
    assert_runs_with_output(
        &list_program(
            "fn mk(a [int]) ([int], int): (a, 1)\n",
            "    var a = List([1, 2, 3])\n    mk(a).0.resize(6, 9)\n    mk(a).0.push(1)\n    println(f\"{a.length()}\")\n",
        ),
        "3",
    );
}

/// A class instance is a reference, so the push lands in the instance the
/// call handed back — the one `bx` names — and not in the list `keep` shares.
#[test]
fn test_a_writer_on_a_field_of_a_returned_instance_writes_the_instance() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Box
    public var items [int]
    public fn init()
        self.items = List([1, 2, 3])

fn pass(b Box) Box: b

fn main()
    let bx = Box()
    var keep = bx.items
    pass(bx).items.push(4)
    println(f"{keep[0]} {keep.length()} {bx.items[0]} {bx.items.length()}")
"#,
        "1 3 1 4",
    );
}

/// Each instance a call hands back is released once the write into it is
/// done, so its drop hook runs once per instance.
#[test]
fn test_a_writer_on_a_returned_instance_runs_its_drop_hook_once() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Box
    public var items [int]
    public fn init()
        self.items = List([1, 2, 3])
    public fn drop(self)
        println("dropped")

fn make() Box: Box()

fn main()
    make().items.push(4)
    make().items.clear()
    println("end")
"#,
        "dropped\ndropped\nend",
    );
}

#[test]
fn test_a_writer_on_a_conditional_leaves_both_branches_alone() {
    assert_runs_with_output(
        &list_program(
            "",
            "    var a = List([1, 2, 3])\n    var b = List([4])\n    let c = true\n    (if c: a else: b).push(9)\n    (if c: a else: b).resize(6, 0)\n    println(f\"{a.length()} {b.length()}\")\n",
        ),
        "3 1",
    );
}

#[test]
fn test_a_writer_on_a_match_over_a_payload_leaves_the_payload_alone() {
    assert_runs_with_output(
        &list_program(
            "",
            "    var a = List([1, 2, 3])\n    let o = Some(a)\n    (match o: Some(x): x, None: List<int>()).push(9)\n    println(f\"{a.length()}\")\n",
        ),
        "3",
    );
}

/// A map lookup hands back a borrow of the entry; the conditional's result
/// holds a reference of its own, and the write lands in a copy of it.
#[test]
fn test_a_writer_on_a_conditional_over_a_map_lookup_leaves_the_entry_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

fn main()
    var m = Map<String, [int]>()
    m.set("k", List([1, 2, 3]))
    var b = List([4])
    let c = true
    (if c: m["k"] else: b).push(9)
    println(f"{m['k'].length()} {b.length()}")
"#,
        "3 1",
    );
}

#[test]
fn test_a_writer_on_a_call_result_in_a_loop_leaves_the_argument_alone() {
    assert_runs_with_output(
        &list_program(
            MK,
            "    var a = List([1, 2, 3])\n    for i in 0..100\n        mk(a).push(i)\n    println(f\"{a.length()}\")\n",
        ),
        "3",
    );
}

#[test]
fn test_struct_and_payload_receivers_keep_their_copy_on_write() {
    assert_runs_with_output(
        &list_program(
            "struct S\n    items [int]\n",
            "    var a = List([1, 2, 3])\n    var s = S(items: a)\n    var s2 = s\n    s.items.resize(6, 9)\n    s.items.push(1)\n    var o = Some(a)\n    match o\n        Some(xs):\n            xs.push(9)\n            println(f\"{a.length()} {s.items.length()} {s2.items.length()} {xs.length()}\")\n        None: println(\"none\")\n",
        ),
        "3 7 3 4",
    );
}
