// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `replace(place, new)` puts `new` in a place and hands back the value the
//! place held, owned, in one step: the place keeps exactly one reference, the
//! caller owns the old value, and nothing is retained or released in between.
//! A counted value neither leaks nor is freed twice, and a drop hook runs
//! once, where the value handed back ends. The place is an `out` argument, so
//! anything but a `var` is refused with the `out` diagnostic.

use super::super::utils::*;

const CONN: &str = r#"
use system.io
use system.memory

class Conn
    var id int
    fn drop()
        println(f"closed {self.id}")
"#;

fn with_conn(program: &str) -> String {
    format!("{CONN}\n{program}")
}

#[test]
fn replace_hands_back_a_string_and_leaves_the_new_one() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

fn main()
    var name = "old"
    let previous = replace(name, "new")
    println(f"{previous} {name}")
"#,
        "old new",
    );
}

#[test]
fn replace_hands_back_an_int() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

fn main()
    var n = 1
    let previous = replace(n, 2)
    println(f"{previous} {n}")
"#,
        "1 2",
    );
}

#[test]
fn replace_hands_back_a_value_held_as_its_bytes() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

struct Point
    x int
    y int

fn main()
    var p = Point(x: 1, y: 2)
    let previous = replace(p, Point(x: 3, y: 4))
    println(f"{previous.x} {previous.y} {p.x} {p.y}")
"#,
        "1 2 3 4",
    );
}

#[test]
fn replace_hands_back_an_optional_string() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

fn main()
    var name String? = Some("old")
    let previous = replace(name, None)
    match previous
        Some(s): println(s)
        None: println("none")
    match name
        Some(s): println(s)
        None: println("none")
"#,
        "old\nnone",
    );
}

#[test]
fn replace_runs_the_old_values_hook_where_it_ends() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var c = Conn(id: 1)
    let old = replace(c, Conn(id: 2))
    println(f"swapped {old.id} for {c.id}")
"#,
        ),
        "swapped 1 for 2\nclosed 1\nclosed 2",
    );
}

#[test]
fn a_discarded_old_value_is_released_at_once() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var c = Conn(id: 1)
    replace(c, Conn(id: 2))
    println(f"now {c.id}")
"#,
        ),
        "closed 1\nnow 2\nclosed 2",
    );
}

#[test]
fn replace_writes_through_an_out_parameter() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn renew(c out Conn) Conn
    return replace(c, Conn(id: 9))

fn main()
    var c = Conn(id: 1)
    let old = renew(c)
    println(f"{old.id} {c.id}")
"#,
        ),
        "1 9\nclosed 1\nclosed 9",
    );
}

#[test]
fn replace_is_generic_over_the_value_it_moves() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

class Named
    var name String

fn swap_in<T>(slot out T, value T) T
    return replace(slot, value)

fn main()
    var n = 1
    var s = "a"
    var c = Named(name: "x")
    var o String? = None
    let n0 = swap_in(n, 2)
    let s0 = swap_in(s, "b")
    let c0 = swap_in(c, Named(name: "y"))
    let o0 = swap_in(o, Some("z"))
    println(f"{n0} {n} {s0} {s} {c0.name} {c.name} {o0 ?? '-'} {o ?? '-'}")
"#,
        "1 2 a b x y - z",
    );
}

#[test]
fn replace_refuses_a_value_that_is_not_a_place() {
    assert_compiler_error(
        r#"
use system.memory

fn make() String
    return "a"

fn main()
    let old = replace(make(), "b")
"#,
        "expected mutable variable for 'out' parameter 'place', but got a non-variable expression",
    );
}

#[test]
fn replace_refuses_a_let_binding() {
    assert_compiler_error(
        r#"
use system.memory

fn main()
    let name = "a"
    let old = replace(name, "b")
"#,
        "'name' is immutable",
    );
}

#[test]
fn replace_moves_a_list_of_narrow_optionals() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory
use system.collections.list

fn main()
    var xs List<i32?> = List<i32?>()
    xs.push(Some(1))
    xs.push(Some(2))
    var ys List<i32?> = List<i32?>()
    let old = replace(xs, ys)
    println(f"{old.length()} {xs.length()}")
"#,
        "2 0",
    );
}

#[test]
fn replace_moves_a_list_of_optional_ints() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory
use system.collections.list

fn main()
    var xs List<int?> = List<int?>()
    xs.push(Some(1))
    var ys List<int?> = List<int?>()
    let old = replace(xs, ys)
    println(f"{old.length()} {xs.length()}")
"#,
        "1 0",
    );
}

#[test]
fn replace_moves_a_list_of_narrow_integers() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory
use system.collections.list

fn main()
    var xs List<i32> = List<i32>()
    xs.push(1)
    var ys List<i32> = List<i32>()
    let old = replace(xs, ys)
    println(f"{old.length()} {xs.length()} {old[0]}")
"#,
        "1 0 1",
    );
}

#[test]
fn replace_moves_a_list_of_optional_strings() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory
use system.collections.list

fn main()
    var xs List<String?> = List<String?>()
    xs.push(Some("a"))
    var ys List<String?> = List<String?>()
    ys.push(None)
    ys.push(Some("b"))
    let old = replace(xs, ys)
    println(f"{old.length()} {xs.length()} {old[0] ?? '-'} {xs[1] ?? '-'}")
"#,
        "1 2 a b",
    );
}

#[test]
fn replace_moves_a_struct_of_bytes() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

struct Rgb
    r u8
    g u8
    b u8

fn main()
    var c = Rgb(r: 1, g: 2, b: 3)
    let old = replace(c, Rgb(r: 4, g: 5, b: 6))
    println(f"{old.r} {old.g} {old.b} {c.r} {c.g} {c.b}")
"#,
        "1 2 3 4 5 6",
    );
}

#[test]
fn a_generic_enum_method_replaces_its_receiver() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

enum Slot<T>
    Full(T)
    Empty

    fn swap(v Slot<T>) Slot<T>
        return replace(self, v)

fn show(s Slot<String>) String
    match s
        Slot.Full(v): return v
        Slot.Empty: return "empty"

fn main()
    var a Slot<String> = Slot.Full("a")
    let old = a.swap(Slot.Empty)
    println(f"{show(old)} {show(a)}")
    var n Slot<int> = Slot.Full(1)
    let m = n.swap(Slot.Full(2))
    match m
        Slot.Full(v): println(f"{v}")
        Slot.Empty: println("empty")
"#,
        "a empty\n1",
    );
}

#[test]
fn a_class_method_may_not_hand_its_receiver_to_an_out_parameter() {
    let program = r#"
use system.memory

class Box
    var n int

    fn reset()
        let old = replace(self, Box(n: 0))
"#;
    assert_compiler_error(program, "MER_TYP_091");
    assert_compiler_error(program, "keeps its state in its fields");
}

#[test]
fn replace_refuses_a_value_of_another_type() {
    assert_compiler_error(
        r#"
use system.memory

fn main()
    var a = 1
    let r = replace(a, "x")
"#,
        "MER_TYP_002",
    );
}

#[test]
fn replace_refuses_one_resource_as_both_the_place_and_the_new_value() {
    let program = &with_conn(
        r#"
fn main()
    var c = Conn(id: 1)
    let o = replace(c, c)
"#,
    );
    assert_compiler_error(program, "MER_OWN_003");
    assert_compiler_error(program, "pass 'c' once");
}

#[test]
fn replace_leaves_an_alias_holding_the_old_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.memory

fn main()
    var ss = "a"
    var sq = ss
    let old = replace(ss, "z")
    println(f"{old} {sq} {ss}")
"#,
        "a a z",
    );
}

#[test]
fn a_generic_runtime_declaration_states_what_it_does_with_each_value() {
    let program = r#"
use system.io

runtime "core" fn bad<T, U>(place out T, new T) T

fn main()
    var a = 1
    println(f"{bad(a, 2)}")
"#;
    assert_compiler_error(
        program,
        "`bad` states nothing about what it does with `place`",
    );
    assert_compiler_error(program, "Cannot infer type argument `U` of `bad`");
}

#[test]
fn a_runtime_declaration_takes_no_out_parameter_it_does_not_overwrite() {
    assert_compiler_error(
        r#"
use system.io

runtime "core" fn miri_rt_swap_probe(place out int, new int) int

fn main()
    var a = 1
    println(f"{miri_rt_swap_probe(a, 2)}")
"#,
        "`miri_rt_swap_probe` takes `place` `out`",
    );
}
