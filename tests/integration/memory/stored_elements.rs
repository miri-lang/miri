// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// An element a container keeps is handed one reference of its own at the call
// that stores it, whatever the element was read from: a local, a temporary, a
// match-arm binding, a parameter or a field. Each run is under the heap guard,
// which reports a read of a freed block and a block left behind at exit, so a
// missing reference (a double free) and a spare one (a leak) both fail.

use super::super::utils::*;

#[test]
fn map_set_keeps_a_string_key_and_value_read_from_locals() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = Map<String, String>()
    let k = "ke" + "y"
    let v = "val" + "ue"
    m.set(k, v)
    m[k + "2"] = v + "2"
    let first = m.get(k) ?? "none"
    let second = m.get("key2") ?? "none"
    println(f"{m.length()} {k} {v} {first} {second}")
"#,
        "2 key value value value2",
    );
}

#[test]
fn map_set_keeps_a_string_key_and_value_built_as_temporaries() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn made(n int) String: f"made{n}"

fn main()
    var m = Map<String, String>()
    m.set("a" + "b", made(1))
    m["c" + "d"] = made(2)
    let ab = m.get("ab") ?? "none"
    let cd = m.get("cd") ?? "none"
    println(f"{m.length()} {ab} {cd}")
"#,
        "2 made1 made2",
    );
}

#[test]
fn map_set_keeps_a_match_arm_binding() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn lookup(hit bool) String?
    if hit: return "bou" + "nd"
    return None

fn main()
    var m = Map<String, String>()
    match lookup(true)
        Some(s): m.set(s, s + "!")
        None: println("none")
    let got = m.get("bound") ?? "none"
    println(f"{m.length()} {got}")
"#,
        "1 bound!",
    );
}

#[test]
fn map_set_keeps_a_parameter() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn store(m out Map<String, String>, k String, v String)
    m.set(k, v)
    m[v] = k

fn main()
    var m = Map<String, String>()
    let k = "pa" + "ram"
    store(m, k, "x" + "y")
    let by_key = m.get(k) ?? "none"
    let by_value = m.get("xy") ?? "none"
    println(f"{m.length()} {by_key} {by_value} {k}")
"#,
        "2 xy param param",
    );
}

#[test]
fn map_set_keeps_a_field_read_off_a_struct() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

struct Person
    name String
    city String

fn main()
    var m = Map<String, String>()
    let p = Person(name: "A" + "da", city: "Lon" + "don")
    m.set(p.name, p.city)
    m[p.city] = p.name
    let city = m.get("Ada") ?? "none"
    println(f"{m.length()} {city} {p.name} {p.city}")
"#,
        "2 London Ada London",
    );
}

#[test]
fn set_add_keeps_one_reference_and_a_duplicate_keeps_none() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = Set<String>()
    let e = "el" + "em"
    let first = s.add(e)
    let again = s.add("el" + "em")
    let other = s.add("ot" + "her")
    let held = s.contains("elem")
    println(f"{s.length()} {first} {again} {other} {e} {held}")
"#,
        "2 true false true elem true",
    );
}

#[test]
fn list_push_keeps_a_local_a_temporary_and_a_parameter() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn push_onto(l out List<String>, item String)
    l.push(item)

fn main()
    var l = List<String>()
    let s = "lo" + "cal"
    l.push(s)
    l.push("tem" + "porary")
    push_onto(l, s)
    println(f"{l.length()} {l[0]} {l[1]} {l[2]} {s}")
"#,
        "3 local temporary local local",
    );
}

#[test]
fn a_list_pushed_into_a_list_of_lists_is_a_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    var outer = List<List<String>>()
    var inner = List<String>()
    inner.push("a" + "b")
    outer.push(inner)
    inner.push("c" + "d")
    outer.push(inner)
    println(f"{outer.length()} {outer[0].length()} {outer[1].length()} {inner.length()} {outer[1][1]}")
"#,
        "2 1 2 2 cd",
    );
}

#[test]
fn fifty_entries_stored_in_a_loop_are_each_kept_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map
use system.collections.list

fn main()
    var m = Map<String, String>()
    var l = List<String>()
    var i = 0
    while i < 50
        let key = f"k{i}"
        m.set(key, f"v{i}")
        l.push(key)
        i = i + 1
    let last = m.get("k49") ?? "none"
    println(f"{m.length()} {l.length()} {last} {l[49]}")
"#,
        "50 50 v49 k49",
    );
}

#[test]
fn a_stored_key_is_the_value_before_a_sibling_argument_writes_it() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn bump(x out int) int
    x = x + 1
    return 7

fn grow(s out String) String
    s = s + "!"
    return "v"

fn main()
    var m = Map<int, int>()
    var a = 1
    m.set(a, bump(a))
    let one = m.get(1) ?? -1
    var n = Map<String, String>()
    var k = "k" + "0"
    n.set(k, grow(k))
    let old = n.get("k0") ?? "none"
    let new = n.get("k0!") ?? "none"
    println(f"{one} {old} {new} {a} {k}")
"#,
        "7 v none 2 k0!",
    );
}

#[test]
fn a_loop_counter_stored_in_a_list_still_guards_the_indexes_it_bounds() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List<int>()
    var i = 0
    while i < 100
        l.push(i)
        i = i + 1
    var total = 0
    i = 0
    while i < l.length()
        total = total + l[i]
        l.push(i)
        i = i + 1
        if i >= 100: break
    println(f"{l.length()} {total}")
"#,
        "200 4950",
    );
}
