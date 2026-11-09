// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An element written from a field or tuple-element read of a local takes a
//! reference only when the value written is itself counted. The element's
//! type says so, not the collection that holds it.

use super::utils::*;

#[test]
fn test_index_assign_struct_field_into_array() {
    assert_runs_with_output(
        r#"
struct P
    a int

fn main()
    let p = P(a: 5)
    var arr [int; 3] = [1, 2, 3]
    arr[1] = p.a
    println(f"{arr[1]} {p.a}")
"#,
        "5 5",
    );
}

#[test]
fn test_index_assign_class_field_into_array() {
    assert_runs_with_output(
        r#"
class P
    var a int
    fn init(a int)
        self.a = a

fn main()
    let p = P(5)
    var arr [int; 3] = [1, 2, 3]
    arr[1] = p.a
    println(f"{arr[1]} {p.a}")
"#,
        "5 5",
    );
}

#[test]
fn test_index_assign_tuple_element_into_array() {
    assert_runs_with_output(
        r#"
fn main()
    let t = (5, 6)
    var arr [int; 3] = [1, 2, 3]
    arr[1] = t.0
    arr[2] = t.1
    println(f"{arr[1]} {arr[2]} {t.0}")
"#,
        "5 6 5",
    );
}

#[test]
fn test_index_assign_struct_field_into_list() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct P
    a int

fn main()
    let p = P(a: 5)
    var l = List([1, 2, 3])
    l[1] = p.a
    println(f"{l[1]} {p.a}")
"#,
        "5 5",
    );
}

#[test]
fn test_index_assign_class_field_into_list() {
    assert_runs_with_output(
        r#"
use system.collections.list

class P
    var a int
    fn init(a int)
        self.a = a

fn main()
    let p = P(5)
    var l = List([1, 2, 3])
    l[1] = p.a
    println(f"{l[1]} {p.a}")
"#,
        "5 5",
    );
}

#[test]
fn test_index_assign_tuple_element_into_list() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let t = (5, 6)
    var l = List([1, 2, 3])
    l[1] = t.1
    println(f"{l[1]} {t.1}")
"#,
        "6 6",
    );
}

#[test]
fn test_index_assign_float_and_bool_fields() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct P
    x float
    ok bool

fn main()
    let p = P(x: 1.5, ok: true)
    var fs = List([0.0, 0.0])
    fs[1] = p.x
    var bs = List([false, false])
    bs[0] = p.ok
    println(f"{fs[1]} {bs[0]}")
"#,
        "1.5 true",
    );
}

#[test]
fn test_index_assign_nested_struct_field_source() {
    assert_runs_with_output(
        r#"
struct Q
    a int

struct P
    q Q

fn main()
    let p = P(q: Q(a: 7))
    var arr [int; 2] = [1, 2]
    arr[0] = p.q.a
    println(f"{arr[0]} {p.q.a}")
"#,
        "7 7",
    );
}

#[test]
fn test_index_assign_nested_struct_value_into_array() {
    assert_heap_guard_output(
        r#"
struct Q
    a int

struct P
    q Q

fn main()
    let p = P(q: Q(a: 7))
    var arr [Q; 2] = [Q(a: 1), Q(a: 2)]
    arr[1] = p.q
    println(f"{arr[1].a} {p.q.a}")
"#,
        "7 7",
    );
}

#[test]
fn test_index_assign_element_field_source() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct P
    a int

fn main()
    let ps = List([P(a: 4), P(a: 9)])
    var arr [int; 2] = [1, 2]
    arr[1] = ps[1].a
    println(f"{arr[1]}")
"#,
        "9",
    );
}

#[test]
fn test_index_assign_element_from_element() {
    assert_runs_with_output(
        r#"
fn main()
    let xs [int; 2] = [8, 9]
    var arr [int; 2] = [1, 2]
    arr[0] = xs[1]
    println(f"{arr[0]}")
"#,
        "9",
    );
}

#[test]
fn test_index_assign_string_field_into_array() {
    assert_heap_guard_output(
        r#"
struct P
    s String

fn main()
    let p = P(s: "new" + "er")
    var arr [String; 2] = ["a" + "b", "c" + "d"]
    arr[1] = p.s
    println(f"{arr[1]} {p.s} {arr[0]}")
"#,
        "newer newer ab",
    );
}

#[test]
fn test_index_assign_string_class_field_into_list() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class P
    var s String
    fn init(s String)
        self.s = s

fn main()
    let p = P("new" + "er")
    var l = List(["a" + "b", "c" + "d"])
    l[0] = p.s
    println(f"{l[0]} {p.s}")
"#,
        "newer newer",
    );
}

#[test]
fn test_index_assign_string_tuple_element_into_list() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let t = ("new" + "er", 1)
    var l = List(["a" + "b"])
    l[0] = t.0
    println(f"{l[0]} {t.0}")
"#,
        "newer newer",
    );
}

#[test]
fn test_index_assign_list_field_into_list() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct P
    xs [int]

fn main()
    let p = P(xs: List([4, 5]))
    var l = List([List([1]), List([2])])
    l[1] = p.xs
    println(f"{l[1][1]} {p.xs[0]} {l[0][0]}")
"#,
        "5 4 1",
    );
}

#[test]
fn test_index_assign_field_sources_in_a_loop() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct P
    a int
    s String

fn main()
    let p = P(a: 3, s: "lo" + "op")
    var nums = List([0, 0, 0])
    var strs = List(["", "", ""])
    for i in 0..3
        nums[i] = p.a
        strs[i] = p.s
    println(f"{nums[0]}{nums[2]} {strs[0]} {strs[2]} {p.s}")
"#,
        "33 loop loop loop",
    );
}

#[test]
fn test_compound_index_assign_from_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct P
    a int

class C
    var a int
    fn init(a int)
        self.a = a

fn main()
    let p = P(a: 5)
    let c = C(2)
    var arr [int; 2] = [1, 2]
    arr[1] += p.a
    arr[1] -= c.a
    var l = List([10])
    l[0] *= p.a
    println(f"{arr[1]} {l[0]}")
"#,
        "5 50",
    );
}

#[test]
fn test_field_target_from_scalar_field_source() {
    assert_runs_with_output(
        r#"
struct P
    a int

class C
    var b int
    fn init()
        self.b = 0

fn main()
    let p = P(a: 5)
    var q = C()
    q.b = p.a
    println(f"{q.b} {p.a}")
"#,
        "5 5",
    );
}

#[test]
fn test_map_index_target_from_field_sources() {
    assert_heap_guard_output(
        r#"
use system.collections.map

struct P
    a int
    s String

fn main()
    let p = P(a: 5, s: "va" + "lue")
    var ints = Map<String, int>()
    ints["k"] = p.a
    var strs = Map<String, String>()
    strs["k"] = p.s
    let i = ints["k"]
    let s = strs["k"]
    println(f"{i} {s} {p.s}")
"#,
        "5 value value",
    );
}

#[test]
fn test_index_assign_generic_class_field_keeps_counted_element() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Box<T>
    var value T
    fn init(value T)
        self.value = value

fn main()
    let b = Box<String>("gen" + "eric")
    var l = List(["a" + "b"])
    l[0] = b.value
    println(f"{l[0]} {b.value}")
    let n = Box<int>(4)
    var m = List([1])
    m[0] = n.value
    println(f"{m[0]}")
"#,
        "generic generic\n4",
    );
}
