// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A range is the stdlib struct `Range<T>` with the public fields `start`,
//! `end` and `inclusive`: range syntax builds one, its fields are read and
//! written like any struct's, and it is compared and hashed by its fields, so
//! it is a set element and a map key like any other struct.

use super::utils::*;

#[test]
fn a_range_reads_its_fields() {
    assert_heap_guard_output(
        r#"
fn main()
    let r = 2..=5
    let h = 7..9
    println(f"{r.start} {r.end} {r.inclusive} {h.inclusive}")
"#,
        "2 5 true false",
    );
}

#[test]
fn a_range_constructed_by_its_fields_equals_the_written_range() {
    assert_heap_guard_output(
        r#"
fn main()
    let r = Range(start: 0, end: 3, inclusive: false)
    println(f"{r == 0..3} {r == 0..=3} {Range(start: 1, end: 4, inclusive: true) == 1..=4}")
"#,
        "true false true",
    );
}

#[test]
fn a_range_bound_by_var_takes_a_field_write() {
    assert_heap_guard_output(
        r#"
fn main()
    var q = 0..3
    q.end = 5
    var total = 0
    for i in q
        total = total + i
    println(f"{q.end} {total}")
"#,
        "5 10",
    );
}

#[test]
fn a_range_bound_by_let_refuses_a_field_write() {
    assert_compiler_error(
        r#"
fn main()
    let p = 0..3
    p.end = 5
"#,
        "MER_TYP_042",
    );
}

#[test]
fn a_set_of_ranges_holds_equal_ranges_once() {
    assert_heap_guard_output(
        r#"
use system.collections.set

fn span(a int, b int) Range<int>
    a..b

fn main()
    var s = Set<Range<int>>()
    s.add(span(0, 3))
    s.add(0..3)
    s.add(0..=3)
    s.add(span(0, 3))
    println(f"{s.length()} {s.contains(0..3)} {s.contains(span(0, 3))} {s.contains(1..3)}")
"#,
        "2 true true false",
    );
}

#[test]
fn a_map_keyed_by_ranges_replaces_the_value_of_an_equal_range() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn span(a int, b int) Range<int>
    a..b

fn main()
    var m = Map<Range<int>, String>()
    m.set(span(0, 3), "a")
    m.set(0..3, "b")
    m.set(0..=3, "c")
    let got = m.get(span(0, 3))
    match got
        Some(v): println(f"{m.length()} {v}")
        None: println("none")
"#,
        "2 b",
    );
}

#[test]
fn a_set_of_tuples_holding_ranges_holds_equal_tuples_once() {
    assert_heap_guard_output(
        r#"
use system.collections.set

fn span(a int, b int) Range<int>
    a..b

fn main()
    var s = Set<(Range<int>, int)>()
    s.add((span(0, 3), 1))
    s.add((0..3, 1))
    s.add((0..3, 2))
    println(f"{s.length()} {s.contains((span(0, 3), 2))}")
"#,
        "2 true",
    );
}

#[test]
fn ranges_of_strings_compare_by_their_bounds() {
    assert_heap_guard_output(
        r#"
fn main()
    let a = "a".."c"
    let same = 'a'..'c' == 'a'..'c'
    println(f"{same} {a == 'a'..'c'} {a == 'a'..='c'} {a.end}")
"#,
        "true true false c",
    );
}

#[test]
fn a_returned_range_keeps_its_managed_bounds_alive() {
    assert_heap_guard_output(
        r#"
class Foo
    v int

fn around(s Foo) Range<Foo>
    return s..s

fn main()
    let r = around(Foo(v: 4))
    println(f"{r.start.v} {r.end.v} {r.inclusive}")
"#,
        "4 4 false",
    );
}

/// A range returned from a parameter holds the parameter's value as a field,
/// the way a struct built from it does, so the call leaves the caller's
/// argument usable: the range literal, the `Range` constructor and a user
/// struct all follow one rule.
#[test]
fn a_range_built_from_a_parameter_and_returned_leaves_the_argument_usable() {
    let bodies = [
        "return xs..xs",
        "return xs..=xs",
        "return Range(start: xs, end: xs, inclusive: false)",
    ];
    for body in bodies {
        assert_runs_with_output(
            &format!(
                r#"
use system.collections.list

fn around(xs [int]) Range<[int]>
    {body}

fn main()
    let items = List([1, 2, 3])
    let w = around(items)
    items.push(4)
    println(f"{{items.length()}} {{w.start.length()}} {{w.end.length()}}")
"#
            ),
            "4 3 3",
        );
    }
}

/// The user struct the range rule matches.
#[test]
fn a_struct_built_from_a_parameter_and_returned_leaves_the_argument_usable() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Pair
    a [int]
    b [int]

fn around(xs [int]) Pair
    return Pair(a: xs, b: xs)

fn main()
    let items = List([1, 2, 3])
    let w = around(items)
    items.push(4)
    println(f"{items.length()} {w.a.length()} {w.b.length()}")
"#,
        "4 3 3",
    );
}

#[test]
fn a_program_cannot_declare_its_own_range() {
    let declarations = [
        "struct Range\n    lo int\n",
        "class Range\n    lo int\n",
        "enum Range\n    Lo\n",
        "trait Range\n    fn lo() int\n",
    ];
    for declaration in declarations {
        assert_compiler_error(
            &format!("{declaration}\nfn main()\n    println(\"x\")\n"),
            "MER_TYP_044",
        );
    }
}

#[test]
fn a_bare_range_type_needs_its_element_type() {
    assert_compiler_error(
        r#"
fn total(_r Range) int
    return 0

fn main()
    println(f"{total(0..3)}")
"#,
        "MER_TYP_036",
    );
}

#[test]
fn the_lowercase_range_type_name_is_unknown() {
    assert_compiler_error(
        r#"
fn total(_r range) int
    return 0

fn main()
    println(f"{total(0..3)}")
"#,
        "Unknown type: range",
    );
}

#[test]
fn a_range_constructor_missing_a_field_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let r = Range(start: 0, end: 3)
    println(f"{r.end}")
"#,
        "MER_TYP_030",
    );
}

#[test]
fn a_range_constructor_naming_an_unknown_field_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let r = Range(start: 0, end: 3, inclusive: false, step: 1)
    println(f"{r.end}")
"#,
        "MER_TYP_033",
    );
}

#[test]
fn a_range_constructor_with_a_field_of_the_wrong_type_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let r = Range(start: 0, end: 3, inclusive: 1)
    println(f"{r.end}")
"#,
        "MER_TYP_002",
    );
}

#[test]
fn a_range_field_write_through_a_parameter_is_refused() {
    assert_compiler_error(
        r#"
fn bump(r Range<int>)
    r.end = 5

fn main()
    bump(0..3)
"#,
        "MER_TYP_042",
    );
}

#[test]
fn a_range_is_not_interpolated_into_a_string() {
    assert_compiler_error(
        r#"
fn main()
    let r = 0..3
    println(f"{r}")
"#,
        "MER_TYP_067",
    );
}

#[test]
fn a_map_keyed_by_ranges_finds_and_removes_by_an_equal_range() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn span(a int, b int) Range<int>
    a..b

fn main()
    var m = Map<Range<int>, String>()
    m.set(span(0, 3), "a")
    m.set(span(1, 4), "b")
    let had = m.contains_key(0..3)
    m.remove(0..3)
    println(f"{had} {m.contains_key(span(0, 3))} {m.contains_key(1..4)} {m.length()}")
"#,
        "true false true 1",
    );
}

#[test]
fn sets_and_maps_of_ranges_of_floats_u64_i8_and_strings_hold_equal_ranges_once() {
    assert_heap_guard_output(
        r#"
use system.collections.set
use system.collections.map

fn fspan(a float, b float) Range<float>
    a..b

fn uspan(a u64, b u64) Range<u64>
    a..b

fn ispan(a i8, b i8) Range<i8>
    a..=b

fn sspan(a String, b String) Range<String>
    a..b

fn main()
    var fs = Set<Range<float>>()
    fs.add(fspan(0.5, 1.5))
    fs.add(0.5..1.5)
    fs.add(0.5..2.5)
    var us = Set<Range<u64>>()
    let ua u64 = 1
    let ub u64 = 9
    us.add(uspan(ua, ub))
    us.add(ua..ub)
    var bs = Set<Range<i8>>()
    let ia i8 = -3
    let ib i8 = 4
    bs.add(ispan(ia, ib))
    bs.add(ia..=ib)
    bs.add(ia..ib)
    var ss = Set<Range<String>>()
    ss.add(sspan("a", "c"))
    ss.add('a'..'c')
    ss.add('a'..='c')
    var fm = Map<Range<float>, int>()
    fm.set(fspan(0.5, 1.5), 1)
    fm.set(0.5..1.5, 2)
    var sm = Map<Range<String>, int>()
    sm.set(sspan("x", "y"), 1)
    sm.set('x'..'y', 2)
    var um = Map<Range<u64>, int>()
    um.set(uspan(ua, ub), 1)
    um.set(ua..ub, 2)
    var im = Map<Range<i8>, int>()
    im.set(ispan(ia, ib), 1)
    im.set(ia..=ib, 2)
    println(f"{fs.length()} {us.length()} {bs.length()} {ss.length()} {fm.length()} {sm.length()} {um.length()} {im.length()}")
"#,
        "2 1 2 2 1 1 1 1",
    );
}

#[test]
fn a_range_is_held_and_written_in_a_generic_function_and_a_generic_class() {
    assert_heap_guard_output(
        r#"
fn widen<T>(r Range<T>, e T) Range<T>
    var w = r
    w.end = e
    return w

class Window<T>
    span Range<T>
    fn first() T
        return self.span.start
    fn covers(other Range<T>) bool
        return self.span == other

fn main()
    let a = widen(0..3, 7)
    let b = widen("a".."b", "z")
    let w = Window<int>(span: 2..=4)
    let s = Window<String>(span: "p".."q")
    println(f"{a.end} {a.start} {b.end} {w.first()} {w.covers(2..=4)} {w.covers(2..4)} {s.first()} {s.covers('p'..'q')}")
"#,
        "7 0 z 2 true false p true",
    );
}
