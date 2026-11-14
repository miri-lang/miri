// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `==` and `!=` on maps compare contents: two maps are equal when they hold
//! the same keys, each with an equal value, whatever order the entries went in.
//! Two maps built separately are never the same object, so comparing them by
//! identity would call every pair unequal.

use super::utils::*;

#[test]
fn maps_with_equal_entries_in_a_different_order_are_equal() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var a = Map<int, int>()
    a[1] = 10
    a[2] = 20
    a[3] = 30
    var b = Map<int, int>()
    b[3] = 30
    b[1] = 10
    b[2] = 20
    println(f"{a == b} {a != b} {b == a}")
"#,
        "true false true",
    );
}

#[test]
fn maps_differing_in_size_key_or_value_are_unequal() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let base = {"a": 1, "b": 2}
    let longer = {"a": 1, "b": 2, "c": 3}
    let other_key = {"a": 1, "c": 2}
    let other_value = {"a": 1, "b": 3}
    println(f"{base == longer} {longer == base} {base == other_key} {base == other_value}")
    println(f"{base != longer} {base != other_key} {base != other_value}")
"#,
        "false false false false\ntrue true true",
    );
}

#[test]
fn empty_maps_are_equal_and_a_map_equals_itself() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let e1 = Map<String, int>()
    let e2 = Map<String, int>()
    let full = {"x": 1}
    println(f"{e1 == e2} {e1 == full} {full == full} {full != full}")
"#,
        "true false true false",
    );
}

#[test]
fn maps_with_list_values_compare_the_lists_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map
use system.collections.list

fn main()
    let a = {"p": List([1, 2]), "q": List([3])}
    let b = {"q": List([3]), "p": List([1, 2])}
    let c = {"p": List([2, 1]), "q": List([3])}
    println(f"{a == b} {a == c}")
"#,
        "true false",
    );
}

#[test]
fn nested_maps_compare_inner_maps_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var a = Map<String, Map<String, int>>()
    a["outer"] = {"x": 1, "y": 2}
    var b = Map<String, Map<String, int>>()
    b["outer"] = {"y": 2, "x": 1}
    var c = Map<String, Map<String, int>>()
    c["outer"] = {"x": 1, "y": 5}
    println(f"{a == b} {a == c}")
"#,
        "true false",
    );
}

#[test]
fn maps_with_struct_values_compare_the_structs_field_by_field() {
    assert_runs_with_output(
        r#"
use system.collections.map

struct Pt
    x int
    y int

fn main()
    var a = Map<int, Pt>()
    a[1] = Pt(x: 1, y: 2)
    a[2] = Pt(x: 3, y: 4)
    var b = Map<int, Pt>()
    b[2] = Pt(x: 3, y: 4)
    b[1] = Pt(x: 1, y: 2)
    var c = Map<int, Pt>()
    c[1] = Pt(x: 1, y: 2)
    c[2] = Pt(x: 3, y: 9)
    println(f"{a == b} {a == c}")
"#,
        "true false",
    );
}

#[test]
fn maps_equal_by_content_hash_alike_whatever_their_order() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let a = {"x": 1, "y": 2, "z": 3}
    let b = {"z": 3, "x": 1, "y": 2}
    let c = {"x": 1, "y": 2, "z": 4}
    println(f"{a.hash() == b.hash()} {a.hash() == c.hash()}")
"#,
        "true false",
    );
}

#[test]
fn comparing_maps_of_counted_values_in_a_loop_releases_what_it_reads() {
    assert_heap_guard_output(
        r#"
use system.collections.map
use system.collections.list

fn main()
    var equal = 0
    var i = 0
    while i < 50
        let a = {"k" + "1": List(["a" + "b"]), "k2": List(["c"])}
        let b = {"k2": List(["c"]), "k1": List(["ab"])}
        let c = {"k1": List(["ab"]), "k2": List(["d"])}
        if a == b: equal += 1
        if a != c: equal += 1
        i += 1
    println(f"{equal}")
"#,
        "100",
    );
}

#[test]
fn comparing_maps_whose_values_have_no_equality_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.map

fn main()
    let a = Map<String, fn(int) int>()
    let b = Map<String, fn(int) int>()
    println(f"{a == b}")
"#,
        "function values cannot be compared with `==`",
    );
}

#[test]
fn a_map_compares_equal_to_an_empty_brace_literal_only_when_empty() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var a = Map<i32, i32>({})
    println(f"{a == {}}")
    a[1] = 2
    println(f"{a == {}}")
"#,
        "true\nfalse",
    );
}

#[test]
fn a_map_of_function_values_never_compared_still_runs() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var ops = Map<String, fn(int) int>()
    ops["double"] = fn(x int) int: x * 2
    match ops.get("double")
        Some(f): println(f"{f(21)}")
        None: println("missing")
"#,
        "42",
    );
}

#[test]
fn a_map_of_class_values_without_equals_never_compared_still_runs() {
    assert_runs_with_output(
        r#"
use system.collections.map

class Foo
    var x int

fn main()
    var m = Map<String, Foo>()
    m["a"] = Foo(x: 7)
    println(f"{m['a'].x} {m.length()}")
"#,
        "7 1",
    );
}

#[test]
fn a_map_of_lists_of_function_values_never_compared_still_runs() {
    assert_runs_with_output(
        r#"
use system.collections.map
use system.collections.list

fn main()
    var m = Map<String, [fn(int) int]>()
    var fs = List<fn(int) int>()
    fs.push(fn(x int) int: x + 1)
    m["inc"] = fs
    println(f"{m['inc'][0](1)}")
"#,
        "2",
    );
}

#[test]
fn a_map_of_optional_values_compares_and_runs() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var a = Map<String, int?>()
    a["x"] = Some(1)
    a["y"] = None
    var b = Map<String, int?>()
    b["y"] = None
    b["x"] = Some(1)
    println(f"{a.length()} {a == b}")
"#,
        "2 true",
    );
}

/// A program struct named like the map's key parameter is the map's value
/// type, not its key type: `Map<String, K>` compares its `K` values field by
/// field, each at its own width.
#[test]
fn a_struct_named_like_the_key_parameter_compares_as_the_value_type() {
    assert_heap_guard_output(
        r#"
use system.collections.map

struct K
    a int
    b float

fn main()
    var x = Map<String, K>()
    x["p"] = K(a: 1, b: 2.5)
    var y = Map<String, K>()
    y["p"] = K(a: 1, b: 2.5)
    var z = Map<String, K>()
    z["p"] = K(a: 1, b: 3.5)
    println(f"{x == y} {x == z} {x['p'].b}")
"#,
        "true false 2.5",
    );
}

/// Program classes named like library type parameters, `K` beside a map's
/// `K` and `T` beside a list's `T`, are held, read and compared as the
/// program's own classes.
#[test]
fn classes_named_like_library_parameters_are_the_programs_classes() {
    assert_heap_guard_output(
        r#"
use system.collections.list
use system.collections.map

class K implements Equatable
    var n int
    public fn equals(other K) bool
        return self.n == other.n

class T
    var s String

fn main()
    var m = Map<String, K>()
    m["a"] = K(n: 3)
    var same = Map<String, K>()
    same["a"] = K(n: 3)
    var xs = List<T>()
    xs.push(T(s: "t" + "1"))
    xs.push(T(s: "t2"))
    var popped = ""
    match xs.pop()
        Some(t): popped = t.s
        None: popped = "none"
    match m.get("a")
        Some(k): println(f"{k.n} {popped} {xs.length()} {m == same}")
        None: println("none")
"#,
        "3 t2 1 true",
    );
}
