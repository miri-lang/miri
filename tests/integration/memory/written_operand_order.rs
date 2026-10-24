// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A write makes its receiver its own only after every other operand of the
//! write is computed, so an operand that hands the receiver's value to
//! another name does so before the check: the check finds the value shared
//! and copies it, the write lands in the copy, and the other name keeps the
//! value as it was.
//!
//! `Holder.grab` stores the list it is handed in the holder, sharing it, and
//! answers a number the write then uses.

use super::super::utils::*;

/// A program declaring `Box` as `box_kind` holding a list, and a `Holder`
/// whose `grab` shares the box's list; `main` builds both and runs `body`.
fn sharing_program(box_kind: &str, body: &str) -> String {
    format!(
        r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

{box_kind} Box
    {field} items [int]

class Holder
    var saved [int]

    fn grab(b Box) int
        self.saved = b.items
        return 9

    fn grab_order(b Box) fn(int, int) int
        self.saved = b.items
        return fn(x int, y int) int: y - x

fn main()
    var b = Box(items: List([1, 3, 2]))
    var h = Holder(saved: List<int>())
{body}
"#,
        field = if box_kind == "class" { "var" } else { "" },
    )
}

#[test]
fn test_an_argument_sharing_a_class_fields_list_leaves_the_shared_value_unwritten() {
    assert_runs_with_output(
        &sharing_program(
            "class",
            "    b.items.push(h.grab(b))\n    println(f\"{h.saved.length()} {b.items.length()}\")",
        ),
        "3 4",
    );
}

#[test]
fn test_an_argument_sharing_a_struct_fields_list_leaves_the_shared_value_unwritten() {
    assert_runs_with_output(
        &sharing_program(
            "struct",
            "    b.items.push(h.grab(b))\n    println(f\"{h.saved.length()} {b.items.length()}\")",
        ),
        "3 4",
    );
}

#[test]
fn test_an_argument_sharing_the_list_a_runtime_writer_sorts_keeps_its_order() {
    assert_runs_with_output(
        &sharing_program(
            "class",
            "    miri_rt_list_sort_by(b.items, h.grab_order(b))\n    \
             println(f\"{h.saved[0]} {h.saved[1]} {h.saved[2]} / {b.items[0]} {b.items[1]} {b.items[2]}\")",
        ),
        "1 3 2 / 3 2 1",
    );
}

#[test]
fn test_an_argument_sharing_the_receiver_of_a_writing_method_needs_no_trap() {
    assert_runs_with_output(
        &sharing_program(
            "class",
            "    b.items.reserve(h.grab(b))\n    b.items.push(4)\n    \
             println(f\"{h.saved.length()} {b.items.length()}\")",
        ),
        "3 4",
    );
}

#[test]
fn test_an_index_sharing_the_list_it_stores_into_leaves_the_shared_value_unwritten() {
    assert_runs_with_output(
        &sharing_program(
            "class",
            "    b.items[h.grab(b) - 9] = 7\n    println(f\"{h.saved[0]} {b.items[0]}\")",
        ),
        "1 7",
    );
}

#[test]
fn test_a_key_sharing_the_map_it_stores_into_leaves_the_shared_value_unwritten() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Box
    var m Map<int, int>

class Holder
    var saved Map<int, int>

    fn grab(b Box) int
        self.saved = b.m
        return 1

fn main()
    var b = Box(m: Map<int, int>())
    var h = Holder(saved: Map<int, int>())
    b.m[h.grab(b)] = 5
    println(f"{h.saved.length()} {b.m.length()}")
"#,
        "0 1",
    );
}

/// A compound map store whose right-hand side gives the field another map
/// reads the old value from the map the field names after it, as an indexed
/// list's compound store does, and writes only that map: the map it was given
/// stays as it was.
#[test]
fn test_a_compound_map_store_whose_right_side_replaces_the_map_writes_only_the_new_one() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Box
    var m {String: int}
    var a {String: int}

    fn share() int
        self.m = self.a
        return 1

fn main()
    var b = Box(m: {"k": 1}, a: {"k": 10})
    b.m["k"] += b.share()
    println(f"a={b.a['k']} m={b.m['k']}")
"#,
        "a=10 m=11",
    );
}

/// A program whose `Holder` shares a `Box`'s list, map or row and answers a
/// number; `main` builds both and runs `body`.
fn holder_program(body: &str) -> String {
    format!(
        r#"
use system.io
use system.collections.list
use system.collections.map

class Box
    var items [int]
    var m {{String: int}}
    var rows [[int]]

class Holder
    var saved [int]
    var smap {{String: int}}
    var srow [int]
    fn grab(b Box) int
        self.saved = b.items
        return 9
    fn grabm(b Box) int
        self.smap = b.m
        return 3
    fn grabrow(b Box) int
        self.srow = b.rows[0]
        return 4

fn main()
    var b = Box(items: List([1, 2, 3]), m: {{"k": 1}}, rows: List([List([1]), List([2])]))
    var h = Holder(saved: List<int>(), smap: {{"z": 0}}, srow: List<int>())
{body}
"#
    )
}

#[test]
fn test_a_compound_map_store_whose_right_side_shares_the_map_leaves_the_shared_value() {
    assert_runs_with_output(
        &holder_program("    b.m[\"k\"] += h.grabm(b)\n    println(f\"{h.smap['k']} {b.m['k']}\")"),
        "1 4",
    );
}

#[test]
fn test_every_store_shape_whose_operand_shares_its_target_leaves_the_shared_value() {
    let body = [
        "    b.m[\"k\"] += h.grabm(b)",
        "    println(f\"{h.smap['k']} {b.m['k']}\")",
        "    b.items[0] += h.grab(b)",
        "    println(f\"{h.saved[0]} {b.items[0]}\")",
        "    var i = 0",
        "    b.rows[i].push(h.grabrow(b))",
        "    println(f\"{h.srow.length()} {b.rows[0].length()}\")",
        "    b.rows[0][0] += h.grabrow(b)",
        "    println(f\"{h.srow[0]} {b.rows[0][0]}\")",
        "    b.m[\"k\"] = h.grabm(b)",
        "    println(f\"{h.smap['k']} {b.m['k']}\")",
        "    b.items[1] = h.grab(b)",
        "    println(f\"{h.saved[1]} {b.items[1]}\")",
    ]
    .join("\n");
    assert_runs_with_output(&holder_program(&body), "1 4\n1 10\n1 2\n1 5\n4 3\n2 9");
}

/// The value combined by an operator the program defines — `String`'s `+` —
/// is computed before the map is made the store's own, and the map's
/// strings are each released once.
#[test]
fn test_a_compound_map_store_of_strings_sharing_the_map_leaves_the_shared_value() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Box
    var m {String: String}

class Holder
    var saved {String: String}

    fn grab(b Box) String
        self.saved = b.m
        return "y"

fn main()
    var b = Box(m: {"k": "x"})
    var h = Holder(saved: {"z": ""})
    b.m["k"] += h.grab(b)
    println(f"{h.saved['k']} {b.m['k']}")
"#,
        "x xy",
    );
}
