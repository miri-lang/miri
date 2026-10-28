// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Results held as their bytes — optionals of scalars, small structs and
//! tuples — come back from a call in registers when their members fit, and
//! through the caller's slot when they do not. Each callee recurses, so it is
//! called rather than inlined, and each program pins the values it prints:
//! a member landed at the wrong offset, or a tag lost on the way back, shows
//! as a wrong number rather than only as a crash.

use super::utils::*;

#[test]
fn an_optional_int_comes_back_from_a_direct_call_present_and_absent() {
    assert_runs_with_output(
        r#"
use system.io

fn find(n int) int?
    if n <= 0
        return None
    if n % 7 == 0
        return n
    find(n - 1)

fn main()
    println(f"{find(20)} {find(5)} {find(7)}")
"#,
        "Some(14) None Some(7)",
    );
}

#[test]
fn an_optional_passed_straight_back_with_return_keeps_its_tag_and_payload() {
    assert_runs_with_output(
        r#"
use system.io

fn find(n int) float?
    if n <= 0
        return None
    if n % 5 == 0
        return (n as float) / 4.0
    return find(n - 1)

fn relay(n int) float?
    return find(n)

fn main()
    println(f"{relay(13)} {relay(4)}")
"#,
        "Some(2.5) None",
    );
}

#[test]
fn a_struct_of_an_int_and_a_float_comes_back_from_a_recursive_call() {
    assert_runs_with_output(
        r#"
use system.io

struct Span
    lo int
    hi float

fn span(n int) Span
    if n == 0
        return Span(lo: 0, hi: 0.5)
    let p = span(n - 1)
    Span(lo: p.lo + n, hi: p.hi * 2.0)

fn main()
    let p = span(4)
    println(f"{p.lo} {p.hi}")
"#,
        "10 8.0",
    );
}

#[test]
fn a_tuple_of_two_ints_comes_back_from_a_recursive_call() {
    assert_runs_with_output(
        r#"
use system.io

fn fib(n int) (int, int)
    if n == 0
        return (0, 1)
    let t = fib(n - 1)
    (t.1, t.0 + t.1)

fn main()
    let t = fib(10)
    println(f"{t.0} {t.1}")
"#,
        "55 89",
    );
}

#[test]
fn an_optional_float_comes_back_from_a_class_method() {
    assert_runs_with_output(
        r#"
use system.io

class Halver
    var floor float

    fn halve(x float, steps int) float?
        if x < self.floor
            return None
        if steps == 0
            return x
        self.halve(x / 2.0, steps - 1)

fn main()
    let h = Halver(floor: 1.0)
    println(f"{h.halve(40.0, 3)} {h.halve(40.0, 6)}")
"#,
        "Some(5.0) None",
    );
}

#[test]
fn an_optional_int_comes_back_from_a_trait_method_called_through_the_trait() {
    assert_runs_with_output(
        r#"
use system.io

trait Seeker
    fn seek(n int) int?

class Sevens implements Seeker
    fn seek(n int) int?
        if n <= 0
            return None
        if n % 7 == 0
            return n
        self.seek(n - 1)

fn main()
    let s Seeker = Sevens()
    println(f"{s.seek(30)} {s.seek(6)}")
"#,
        "Some(28) None",
    );
}

#[test]
fn a_struct_comes_back_from_a_closure_call() {
    assert_runs_with_output(
        r#"
use system.io

struct Span
    lo int
    hi float

fn main()
    let scale = 1.5
    let make = fn(n int) Span
        return Span(lo: n * 3, hi: (n as float) * scale)
    let a = make(2)
    let b = make(-4)
    println(f"{a.lo} {a.hi} {b.lo} {b.hi}")
"#,
        "6 3.0 -12 -6.0",
    );
}

#[test]
fn a_tuple_comes_back_from_a_named_function_called_as_a_value() {
    assert_runs_with_output(
        r#"
use system.io

fn split(n int) (int, float)
    if n < 10
        return (n, 0.25)
    let t = split(n / 10)
    (t.0 + n % 10, t.1 * 2.0)

fn apply(f fn(n int) (int, float), n int) (int, float)
    return f(n)

fn main()
    let t = apply(split, 1234)
    println(f"{t.0} {t.1}")
"#,
        "10 2.0",
    );
}

#[test]
fn a_result_lands_in_a_field_of_a_value_local() {
    assert_runs_with_output(
        r#"
use system.io

struct Slot
    id int
    best int?

fn find(n int) int?
    if n <= 0
        return None
    if n % 7 == 0
        return n
    find(n - 1)

fn main()
    var s = Slot(id: 1, best: None)
    s.best = find(20)
    let before = s.best
    s.best = find(3)
    println(f"{s.id} {before} {s.best}")
"#,
        "1 Some(14) None",
    );
}

#[test]
fn a_result_lands_in_a_struct_field_of_another_held_value() {
    assert_runs_with_output(
        r#"
use system.io

struct Span
    lo int
    hi float

struct Frame
    tag int
    inner Span

fn span(n int) Span
    if n == 0
        return Span(lo: 0, hi: 0.5)
    let p = span(n - 1)
    Span(lo: p.lo + n, hi: p.hi * 2.0)

fn nest(n int) Frame
    if n == 0
        return Frame(tag: 0, inner: span(0))
    let f = nest(n - 1)
    Frame(tag: f.tag + 1, inner: span(n))

fn main()
    var f = nest(3)
    let first = f.inner.lo
    f.inner = span(4)
    println(f"{f.tag} {first} {f.inner.lo} {f.inner.hi}")
"#,
        "3 6 10 8.0",
    );
}

#[test]
fn a_struct_too_wide_for_registers_still_comes_back_through_the_slot() {
    assert_runs_with_output(
        r#"
use system.io

struct Vec3
    x float
    y float
    z float

fn walk(n int) Vec3
    if n == 0
        return Vec3(x: 1.0, y: 2.0, z: 3.0)
    let v = walk(n - 1)
    Vec3(x: v.y, y: v.z, z: v.x + 1.0)

fn main()
    let v = walk(4)
    println(f"{v.x} {v.y} {v.z}")
"#,
        "3.0 4.0 3.0",
    );
}

#[test]
fn a_generic_function_returns_an_optional_at_each_instantiation() {
    assert_runs_with_output(
        r#"
use system.io

fn pick<T>(x T, ok bool) T?
    if ok
        return x
    return None

fn main()
    println(f"{pick(5, true)} {pick(5, false)} {pick(2.5, true)} {pick(true, true)}")
    let inner int? = 4
    match pick(inner, true)
        Some(o): println(f"{o}")
        None: println("none")
"#,
        "Some(5) None Some(2.5) Some(true)\nSome(4)",
    );
}

/// A lambda wrapping a generic call is a function value whose body returns
/// the optional in registers; the caller reaches it through the value.
#[test]
fn a_function_value_wrapping_a_generic_call_returns_its_optional() {
    assert_runs_with_output(
        r#"
use system.io

fn pick<T>(x T, ok bool) T?
    if ok
        return x
    return None

fn apply(f fn(x int, ok bool) int?, ok bool) int?
    return f(7, ok)

fn main()
    let wrapped = fn(x int, ok bool) int?: pick(x, ok)
    println(f"{apply(wrapped, true)} {apply(wrapped, false)}")
"#,
        "Some(7) None",
    );
}

/// A generic class's method reached through its generic trait is a vtable
/// entry of each instantiation, returning a struct or a float optional.
#[test]
fn a_generic_class_method_reached_through_its_trait_returns_its_value() {
    assert_runs_with_output(
        r#"
use system.io

struct Pt
    x int
    y float

trait Getter<T>
    fn get(ok bool) T?

class Box<T> implements Getter<T>
    var v T
    fn get(ok bool) T?
        if ok
            return self.v
        return None

fn main()
    let g Getter<Pt> = Box<Pt>(v: Pt(x: 3, y: 1.5))
    match g.get(true)
        Option.Some(p): println(f"{p.x} {p.y}")
        Option.None: println("none")
    let h Getter<float> = Box<float>(v: 2.5)
    println(f"{h.get(true)} {h.get(false)}")
"#,
        "3 1.5\nSome(2.5) None",
    );
}

#[test]
fn a_generic_class_method_returns_its_parameter_optional_at_each_width() {
    assert_runs_with_output(
        r#"
use system.io

struct Pt
    x int
    y float

class Box<T>
    var v T
    fn get(ok bool) T?
        if ok
            return self.v
        return None

fn main()
    let bi = Box<int>(v: 4)
    let bf = Box<float>(v: 4.5)
    let bp = Box<Pt>(v: Pt(x: 1, y: 2.0))
    let bt = Box<(int, int, int)>(v: (1, 2, 3))
    println(f"{bi.get(true)} {bi.get(false)} {bf.get(true)} {bf.get(false)}")
    match bp.get(true)
        Option.Some(p): println(f"{p.x} {p.y}")
        Option.None: println("none")
    match bt.get(true)
        Option.Some(t): println(f"{t.0} {t.1} {t.2}")
        Option.None: println("none")
    match bt.get(false)
        Option.Some(t): println("bad")
        Option.None: println("none")
"#,
        "Some(4) None Some(4.5) None\n1 2.0\n1 2 3\nnone",
    );
}

/// A trait's default method calls the class's override through `self` and
/// returns a struct; a class overriding the default returns its own.
#[test]
fn a_struct_comes_back_through_a_trait_default_and_its_override() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct Pt
    x int
    y float

trait Maker
    fn make(n int) Pt
    fn twice(n int) Pt
        let p = self.make(n)
        return Pt(x: p.x * 2, y: p.y * 2.0)

class A implements Maker
    fn make(n int) Pt
        return Pt(x: n, y: 0.5)

class B implements Maker
    fn make(n int) Pt
        return Pt(x: -n, y: 1.5)
    fn twice(n int) Pt
        return Pt(x: 100, y: 200.0)

fn main()
    var ms = List<Maker>()
    ms.push(A())
    ms.push(B())
    println(f"{ms[0].make(3).x} {ms[0].make(3).y} {ms[1].make(3).x} {ms[1].make(3).y}")
    println(f"{ms[0].twice(3).x} {ms[0].twice(3).y} {ms[1].twice(3).x} {ms[1].twice(3).y}")
"#,
        "3 0.5 -3 1.5\n6 1.0 100 200.0",
    );
}

#[test]
fn a_closure_capturing_a_struct_returns_a_struct_built_from_it() {
    assert_runs_with_output(
        r#"
use system.io

struct Pt
    x int
    y float

fn mk(n int) Pt
    if n > 0
        return mk(n - 1)
    return Pt(x: 7, y: 0.25)

fn main()
    let base = mk(0)
    let cap = fn(k int) Pt
        return Pt(x: base.x + k, y: base.y * 2.0)
    let a = cap(1)
    println(f"{a.x} {a.y}")
"#,
        "8 0.5",
    );
}

#[test]
fn function_values_held_in_a_list_return_their_structs() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct Pt
    x int
    y float

fn mk(n int) Pt
    if n > 0
        return mk(n - 1)
    return Pt(x: 7, y: 0.25)

fn main()
    let fs List<fn(int) Pt> = List([mk, fn(n int) Pt
        return Pt(x: n, y: 9.0)
    ])
    println(f"{fs[0](3).x} {fs[1](3).y}")
    let k = fn(n int) Pt
        return mk(n)
    println(f"{k(1).x}")
"#,
        "7 9.0\n7",
    );
}

#[test]
fn mutually_recursive_functions_pass_a_tuple_back_and_forth() {
    assert_runs_with_output(
        r#"
use system.io

fn ev(n int) (int, int)
    if n == 0
        return (0, 1)
    let t = od(n - 1)
    (t.0 + 1, t.1)

fn od(n int) (int, int)
    if n == 0
        return (5, 6)
    let t = ev(n - 1)
    (t.1, t.0)

fn sum(t (int, int)) int
    return t.0 * 10 + t.1

fn main()
    println(f"{sum(ev(3))} {sum(od(2))}")
"#,
        "76 66",
    );
}

#[test]
fn results_passed_straight_as_arguments_keep_their_members() {
    assert_runs_with_output(
        r#"
use system.io

struct Pt
    x int
    y float

fn mk(n int) Pt
    if n > 0
        return mk(n - 1)
    return Pt(x: 7, y: 0.25)

fn takes(p Pt, q Pt) float
    return (p.x as float) + p.y + (q.x as float) * 10.0 + q.y

fn main()
    println(f"{takes(mk(1), mk(0))}")
    if mk(1).x == 7
        println("cond ok")
    mk(2)
"#,
        "77.5\ncond ok",
    );
}

#[test]
fn an_optional_comes_back_by_an_early_return_from_a_loop() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn first_neg(xs [int]) int?
    for x in xs
        if x < 0
            return x
    return None

fn main()
    println(f"{first_neg(List([1, 2, -5, 3]))} {first_neg(List([1]))}")
    var l = List<Option<int>>()
    l.push(first_neg(List([-9])))
    l.push(first_neg(List([9])))
    println(f"{l[0]} {l[1]}")
"#,
        "Some(-5) None\nSome(-9) None",
    );
}

#[test]
fn a_nested_optional_comes_back_with_each_layer() {
    assert_runs_with_output(
        r#"
use system.io

fn nest(n int) Option<Option<int>>
    if n == 0
        return None
    if n == 1
        let r Option<int> = None
        return r
    return n

fn main()
    println(f"{nest(0)} {nest(1)} {nest(3)}")
"#,
        "None Some(None) Some(Some(3))",
    );
}

#[test]
fn a_result_assigned_to_an_array_element_keeps_its_members() {
    assert_runs_with_output(
        r#"
use system.io

fn mm(n int) (float, float)
    if n > 0
        return mm(n - 1)
    return (1.5, -2.5)

fn main()
    let m = mm(1)
    println(f"{m.0} {m.1}")
    var arr = [mm(0), mm(1)]
    arr[1] = mm(2)
    println(f"{arr[1].0} {arr[1].1}")
"#,
        "1.5 -2.5\n1.5 -2.5",
    );
}

/// Members narrower than a register — `i8`, `i16`, `bool`, `f32` — come back
/// at their own widths, in registers or, past the budget, through the slot.
#[test]
fn narrow_members_come_back_at_their_own_widths() {
    assert_runs_with_output(
        r#"
use system.io

struct Mixed
    a i8
    b i16
    c bool
    d f32

struct Flags
    p bool
    q bool
    r i8

struct Halves
    x f32
    y f32

fn mixed(n int) Mixed
    if n > 0
        return mixed(n - 1)
    return Mixed(a: -3, b: -300, c: true, d: 1.5)

fn flags(n int) Flags
    if n > 0
        return flags(n - 1)
    return Flags(p: true, q: false, r: -7)

fn halves(n int) Halves
    if n > 0
        return halves(n - 1)
    return Halves(x: 1.25, y: -2.5)

fn spread(n int) (f32, int, f32, i16)
    if n > 0
        return spread(n - 1)
    return (1.5 as f32, 2, 3.5 as f32, -4 as i16)

fn main()
    let m = mixed(2)
    println(f"{m.a} {m.b} {m.c} {m.d}")
    let f = flags(2)
    println(f"{f.p} {f.q} {f.r}")
    let h = halves(2)
    println(f"{h.x} {h.y}")
    let s = spread(2)
    println(f"{s.0} {s.1} {s.2} {s.3}")
"#,
        "-3 -300 true 1.5\ntrue false -7\n1.25 -2.5\n1.5 2 3.5 -4",
    );
}

#[test]
fn two_ints_and_two_floats_fill_the_registers_and_come_back_whole() {
    assert_runs_with_output(
        r#"
use system.io

struct Quad
    a int
    b float
    c int
    d float

fn quad(n int) Quad
    if n > 0
        return quad(n - 1)
    return Quad(a: 1, b: 2.5, c: 3, d: 4.5)

fn main()
    let x = quad(2)
    println(f"{x.a} {x.b} {x.c} {x.d}")
"#,
        "1 2.5 3 4.5",
    );
}

/// Optionals of scalars of every width come back, and ones too wide for a
/// register or counted keep their own way back.
#[test]
fn optionals_of_each_scalar_width_come_back_present() {
    assert_runs_with_output(
        r#"
use system.io

fn ff(n int) f32?
    if n > 0
        return ff(n - 1)
    return 2.5

fn u(n int) u8?
    if n > 0
        return u(n - 1)
    return 200

fn w(n int) i128?
    if n > 0
        return w(n - 1)
    return 5

fn s(n int) String?
    if n > 0
        return s(n - 1)
    return "hi"

fn main()
    match ff(1)
        Option.Some(v): println(f"{v}")
        Option.None: println("none")
    match u(1)
        Option.Some(v): println(f"{v}")
        Option.None: println("none")
    match w(1)
        Option.Some(v): println(f"{v == 5}")
        Option.None: println("none")
    match s(1)
        Option.Some(v): println(v)
        Option.None: println("none")
"#,
        "2.5\n200\ntrue\nhi",
    );
}

/// The comparator the runtime calls for `sorted_by` answers an `int`; one
/// answering an optional, which the runtime could not read back, is refused
/// by its type.
#[test]
fn a_comparator_answering_an_optional_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn main()
    let l = List([3, 1, 2])
    let s = l.sorted_by(fn(a int, b int) int?: a - b)
    println(f"{s[0]}")
"#,
        "Type Mismatch",
    );
}

/// A closure handed straight to a runtime entry point that calls it, but
/// answering a value held as its bytes, is refused when the program is
/// built: the runtime calls it through a trampoline that stores only a word.
#[test]
fn a_runtime_called_closure_answering_a_held_value_is_refused() {
    assert_build_error(
        r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l List<int>, f fn(int, int) int?)

fn main()
    var l = List([3, 1, 2])
    miri_rt_list_sort_by(l, fn(a int, b int) int?: a - b)
    println(f"{l[0]}")
"#,
        "which a trampoline cannot store",
    );
}
