// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A value held as its bytes is passed as the caller's own bytes. These pin
// that no call can see those bytes change under it: through its return slot,
// through an `out` argument, through the holder of a field or an element,
// or through a closure.

use super::super::utils::*;

/// `v = swap_scale(v, ..)`: the call writes `v` through its return slot
/// while it reads `v` as its argument, so it must read a copy.
#[test]
fn a_value_passed_to_the_call_writing_it_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

fn swap_scale(a V, s float) V
    V(x: a.y * s, y: a.x * s)

fn main()
    var v = V(x: 1.0, y: 2.0)
    v = swap_scale(v, 2.0)
    println(f"{v.x} {v.y}")
"#,
        "4.0 2.0",
    );
}

/// `set(v, v)`: the callee writes `v` through its `out` parameter before it
/// reads the value argument, which must still hold the old bytes.
#[test]
fn a_value_passed_beside_itself_as_out_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

fn set(o out V, a V)
    o = V(x: 10.0, y: 20.0)
    o = V(x: a.x + o.x, y: a.y)

fn main()
    var v = V(x: 1.0, y: 2.0)
    set(v, v)
    println(f"{v.x} {v.y}")
"#,
        "11.0 2.0",
    );
}

/// `read_after(b, b.v)`: the callee changes `b.v` through the instance before
/// it reads the value argument taken from that field.
#[test]
fn a_field_passed_by_value_keeps_its_bytes_while_the_holder_changes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

class Box
    var v V

    fn bump()
        self.v = V(x: self.v.x + 100.0, y: self.v.y + 100.0)

fn read_after(b Box, a V) float
    b.bump()
    a.x

fn main()
    let b = Box(v: V(x: 1.0, y: 2.0))
    let r = read_after(b, b.v)
    println(f"{r} {b.v.x}")
"#,
        "1.0 101.0",
    );
}

/// A closure reassigns its own captured `v` while the callee holds the
/// caller's `v` as a value argument; the argument still reads 1.0 after the
/// closure ran, and the caller's `v` is unchanged.
#[test]
fn a_closure_reassigning_its_capture_leaves_the_value_argument_alone() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

fn call_with(a V, f fn() float) float
    let seen = f()
    let after = a.x
    after + seen

fn main()
    var v = V(x: 1.0, y: 2.0)
    let f = fn() float
        v = V(x: 50.0, y: 60.0)
        v.x
    let r = call_with(v, f)
    println(f"{r} {v.x}")
"#,
        "51.0 1.0",
    );
}

/// `first_after(xs, xs[0])`: the callee writes the first element of its own
/// copy of the list before it reads the element it was handed.
#[test]
fn a_list_element_passed_by_value_keeps_its_bytes_while_a_copy_changes() {
    assert_runs_with_output(
        r#"use system.collections.list

struct V
    x float
    y float

fn first_after(xs [V], a V) float
    var ys = xs
    ys[0] = V(x: 99.0, y: 99.0)
    a.x + ys[0].x

fn main()
    let xs = List([V(x: 1.0, y: 2.0), V(x: 3.0, y: 4.0)])
    let r = first_after(xs, xs[0])
    println(f"{r} {xs[0].x}")
"#,
        "100.0 1.0",
    );
}

/// `v = rec(v, 3)` recurses on a value built from its argument and adds
/// the argument back after each return.
#[test]
fn a_recursive_value_call_into_its_own_argument_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

fn rec(a V, n int) V
    if n == 0
        return a
    var b = rec(V(x: a.y, y: a.x), n - 1)
    b = V(x: b.x + a.x, y: b.y)
    b

fn main()
    var v = V(x: 1.0, y: 2.0)
    v = rec(v, 3)
    println(f"{v.x} {v.y}")
"#,
        "6.0 1.0",
    );
}

/// `v = run(t, v)` and `v = t.go(v)`: a call through a trait and a method
/// call, each writing the local it reads.
#[test]
fn a_virtual_call_writing_its_value_argument_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

trait T
    fn go(self, a V) V

class A implements T
    var k float
    fn go(self, a V) V
        V(x: a.y + self.k, y: a.x)

fn run(t T, a V) V
    t.go(a)

fn main()
    var v = V(x: 1.0, y: 2.0)
    let t = A(k: 5.0)
    v = run(t, v)
    v = t.go(v)
    println(f"{v.x} {v.y}")
"#,
        "6.0 7.0",
    );
}

/// `setin(w, w)` on a value holding a value, an optional and an `int`: the
/// callee writes `w.i` through its `out` parameter before reading `w.i.a`.
#[test]
fn a_nested_value_passed_beside_itself_as_out_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct In
    a float
    b float

struct Out
    i In
    o In?
    k int

fn flip(w Out) Out
    Out(i: In(a: w.i.b, b: w.i.a), o: w.o, k: w.k + 1)

fn setin(o out Out, w Out)
    o.i = In(a: 9.0, b: 9.0)
    o.k = w.k + w.i.a as int

fn main()
    var w = Out(i: In(a: 1.0, b: 2.0), o: In(a: 3.0, b: 4.0), k: 0)
    w = flip(w)
    println(f"{w.i.a} {w.i.b} {w.k}")
    w.i = In(a: w.i.b, b: w.i.a)
    setin(w, w)
    println(f"{w.i.a} {w.k}")
"#,
        "2.0 1.0 1\n9.0 2",
    );
}

/// `w.inner = rot(w.inner)` twice, and callees that copy their value
/// parameter into a local and write the copy.
#[test]
fn a_field_receiving_a_call_on_itself_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct In
    a float
    b float

struct W
    inner In
    n int

fn rot(i In) In
    In(a: i.b, b: i.a + 1.0)

fn mut_param(a In) float
    var b = a
    b.a = 100.0
    a.a + b.a

fn write_param(a In) float
    var a2 = a
    a2.b = 7.0
    a.b

fn main()
    var w = W(inner: In(a: 1.0, b: 2.0), n: 0)
    w.inner = rot(w.inner)
    w.inner = rot(w.inner)
    println(f"{w.inner.a} {w.inner.b}")
    let i = In(a: 1.0, b: 2.0)
    println(f"{mut_param(i)} {write_param(i)} {i.a}")
"#,
        "2.0 3.0\n101.0 2.0 1.0",
    );
}

/// `v = pass(v, swap(v, 2.0))`: a generic function instantiated at a value
/// type, called into the local it reads.
#[test]
fn a_generic_call_writing_its_value_argument_reads_the_old_bytes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

fn pass<T>(_a T, b T) T
    b

fn swap(a V, s float) V
    V(x: a.y * s, y: a.x * s)

fn main()
    var v = V(x: 1.0, y: 2.0)
    v = pass(v, swap(v, 2.0))
    v = pass(swap(v, 1.0), v)
    println(f"{v.x} {v.y}")
"#,
        "4.0 2.0",
    );
}

/// `add(v, v)`: one value handed to both parameters of a call, once into a
/// fresh local and once into `v` itself.
#[test]
fn a_value_passed_to_two_parameters_of_one_call_reads_the_same_bytes() {
    assert_runs_with_output(
        r#"
struct V
    x float
    y float

fn add(a V, b V) V
    V(x: a.x + b.y * 10.0, y: a.y + b.x * 10.0)

fn main()
    var v = V(x: 1.0, y: 2.0)
    let s = add(v, v)
    v = add(v, v)
    println(f"{s.x} {s.y} {v.x} {v.y}")
"#,
        "21.0 12.0 21.0 12.0",
    );
}
