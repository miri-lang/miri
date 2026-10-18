// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every place an optional held as its bytes meets a value spelled some other
//! way: a declaration with no initializer, a bare value or the absent value
//! handed to an optional slot — an enum payload, a virtual call's parameter,
//! a collection lookup, a comparison, the default of `??` — and an optional
//! read out of a holder into an optional one layer deeper.
//!
//! Each program runs under the MIR verifier and the heap guard.

use super::super::utils::*;

#[test]
fn test_an_optional_declared_without_a_value_is_absent() {
    assert_heap_guard_output(
        r#"
use system.io

struct P
    a int
    b int

fn pick(n int) int?
    var r int?
    if n > 100000
        r = 5
    return r

fn main()
    var x int?
    println(f"{x ?? 5}")
    var p P?
    println(f"{p == None}")
    var y Option<int?>
    println(f"{y == None}")
    var b bool?
    println(f"{b ?? false}")
    var i = 0
    var z int?
    while i < 3
        if i == 1
            z = i
        i = i + 1
    println(f"{z ?? 99}")
    println(f"{pick(1) ?? 77}")
"#,
        "5\ntrue\ntrue\nfalse\n1\n77",
    );
}

#[test]
fn test_an_enum_payload_declared_optional_wraps_a_bare_value() {
    assert_heap_guard_output(
        r#"
use system.io

struct P
    a int
    b int

struct Big
    a int
    b int
    c int
    d int
    e int
    f int
    g int
    h int
    i int
    j int
    k int
    l int
    m int
    n int
    o int

enum E
    A(int?)
    B(P?, int)
    W(Big?)
    C

fn show(e E) String
    match e
        E.A(x): return f"A {x ?? -1}"
        E.B(p, k)
            match p
                Some(q): return f"B {q.a} {k}"
                None: return f"B none {k}"
        E.W(w)
            match w
                Some(b): return f"W {b.a} {b.o}"
                None: return "W none"
        E.C: return "C"

fn main()
    println(show(E.A(3)))
    println(show(E.A(Some(5))))
    println(show(E.A(None)))
    let v int? = 6
    println(show(E.A(v)))
    println(show(E.B(P(a: 1, b: 2), 3)))
    println(show(E.B(None, 4)))
    println(show(E.W(Big(a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10, k: 11, l: 12, m: 13, n: 14, o: 15))))
    println(show(E.C))
"#,
        "A 3\nA 5\nA -1\nA 6\nB 1 3\nB none 4\nW 1 15\nC",
    );
}

#[test]
fn test_a_virtual_call_through_a_generic_trait_takes_absent_and_bare_values() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    v int

struct P
    x int

trait Source<T>
    fn next() T?
    fn put(x T?) int

class Gen<T> implements Source<T>
    var items List<T>
    fn next() T?
        return self.items.pop()
    fn put(x T?) int
        match x
            Some(v)
                self.items.push(v)
                return 1
            None: return 0

fn use_int(s Source<int>) String
    let e = None
    let a = s.put(Some(4)) + s.put(None) + s.put(e)
    let x = s.next()
    let y = s.next()
    return f"{a} {x ?? -1} {y ?? -1}"

fn use_p(s Source<P>) String
    let a = s.put(P(x: 3)) + s.put(None)
    match s.next()
        Some(p): return f"{a} {p.x}"
        None: return f"{a} none"

fn use_oi(s Source<int?>) String
    let a = s.put(Some(None)) + s.put(None)
    match s.next()
        Some(i): return f"{a} some {i == None}"
        None: return f"{a} none"

fn use_node(s Source<Node>) String
    let a = s.put(Node(v: 2)) + s.put(None)
    match s.next()
        Some(n): return f"{a} {n.v}"
        None: return f"{a} none"

fn use_f(s Source<float>) String
    let a = s.put(2.5) + s.put(None)
    return f"{a} {s.next() ?? 0.0}"

fn main()
    println(use_int(Gen<int>(items: List<int>())))
    println(use_p(Gen<P>(items: List<P>())))
    println(use_oi(Gen<int?>(items: List<int?>())))
    println(use_node(Gen<Node>(items: List<Node>())))
    println(use_f(Gen<float>(items: List<float>())))
"#,
        "1 4 -1\n1 3\n1 some true\n1 2\n1 2.5",
    );
}

#[test]
fn test_a_lookup_of_an_optional_key_takes_absent_and_bare_keys() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set
use system.collections.map

struct P
    a int
    b int

fn main()
    var sk = Set<P?>()
    sk.add(None)
    sk.add(P(a: 1, b: 2))
    println(f"{None in sk} {sk.contains(None)} {P(a: 1, b: 2) in sk} {sk.length()}")
    var mk = Map<int?, String>()
    mk[None] = "n"
    mk[1] = "one"
    mk[Some(2)] = "two"
    println(f"{None in mk} {1 in mk} {3 in mk} {mk.length()}")
    println(mk[None])
    println(mk[2])
    println(mk[Some(1)])
    var s = Set<int?>()
    s.add(1)
    println(f"{1 in s} {Some(1) in s} {2 in s} {None in s}")
"#,
        "true true true 2\ntrue true false 3\nn\ntwo\none\ntrue true false false",
    );
}

#[test]
fn test_an_optional_compares_with_absent_and_bare_values() {
    assert_heap_guard_output(
        r#"
use system.io

struct P
    x int

fn mk() int?
    return 3

fn main()
    let e = None
    let x int? = Some(3)
    let y int? = None
    let p P? = None
    println(f"{x == e} {y == e} {p == e} {x != e}")
    let w = mk()
    let i = 3
    println(f"{w == 3} {w == i} {w != 4} {y == 3}")
"#,
        "false true true true\ntrue true true false",
    );
}

#[test]
fn test_a_coalescing_default_is_wrapped_into_the_optional_it_stands_for() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a Option<int?> = None
    let r = a ?? 5
    println(f"{r ?? -1}")
    let b Option<int?> = Some(None)
    let r2 = b ?? 5
    println(f"{r2 ?? -1}")
    let c Option<int?> = Some(4)
    let r3 int? = c ?? 1
    println(f"{r3 ?? 7}")
    var bs = List<bool?>()
    let d = bs.pop() ?? false
    println(f"{d ?? true}")
"#,
        "5\n-1\n4\nfalse",
    );
}

#[test]
fn test_a_function_reference_takes_and_returns_optionals_held_as_bytes() {
    assert_heap_guard_output(
        r#"
use system.io

struct P
    x int

fn takes(n Option<int>) int
    return n ?? -1

fn takesp(n P?) P?
    return n

fn main()
    let t = takes
    println(f"{t(None)} {t(Some(4))} {t(5)}")
    let tp = takesp
    println(f"{tp(None) == None}")
    match tp(P(x: 2))
        Some(p): println(f"{p.x}")
        None: println("none")
    let e = None
    println(f"{t(e)}")
"#,
        "-1 4 5\ntrue\n2\n-1",
    );
}

#[test]
fn test_an_optional_read_out_of_a_holder_is_wrapped_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct S
    v int?

fn first<T>(xs List<T>) T?
    if xs.length() == 0:
        return None
    return xs[0]

fn f(xs List<int?>) Option<int?>
    return xs[0]

fn main()
    var on = List<int?>()
    on.push(None)
    match first(on)
        Some(inner): println(f"generic some {inner == None}")
        None: println("generic none")
    match f(on)
        Some(inner): println(f"list some {inner == None}")
        None: println("list none")
    var r Option<int?> = on[0]
    match r
        Some(inner): println(f"element some {inner == None}")
        None: println("element none")
    let s = S(v: None)
    var r1 Option<int?> = s.v
    match r1
        Some(inner): println(f"field some {inner == None}")
        None: println("field none")
    let q (int?, int) = (None, 1)
    var r2 Option<int?> = q.0
    match r2
        Some(inner): println(f"tuple some {inner == None}")
        None: println("tuple none")
"#,
        "generic some true\nlist some true\nelement some true\nfield some true\ntuple some true",
    );
}

#[test]
fn test_a_generic_holder_of_an_optional_keeps_its_layers() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    var item T?
    fn take() T?
        let r = self.item
        self.item = None
        return r

struct SBox<T>
    item T?

enum G<T>
    One(T?)
    Two(Option<T?>)

enum E
    A(Option<int?>)
    Z

fn show(o Option<int?>) String
    match o
        Some(inner): return f"some({inner ?? -1})"
        None: return "none"

fn main()
    var b = Box<int?>(item: Some(None))
    println(show(b.item))
    println(show(b.take()))
    println(show(b.item))
    let s = SBox<int?>(item: Some(None))
    let t = SBox<int?>(item: Some(3))
    println(f"{show(s.item)} {show(t.item)}")
    match E.A(Some(None))
        E.A(o): println(show(o))
        E.Z: println("z")
    match G<int?>.One(Some(None))
        G.One(o): println(show(o))
        G.Two(_): println("two")
    match G<int>.Two(Some(None))
        G.One(_): println("one")
        G.Two(o): println(show(o))
"#,
        "some(-1)\nsome(-1)\nnone\nsome(-1) some(3)\nsome(-1)\nsome(-1)\nsome(-1)",
    );
}

#[test]
fn test_an_absent_element_inserted_or_set_keeps_its_layers() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct P
    x int

fn main()
    var xs = List<int?>()
    xs.push(7)
    xs.insert(0, None)
    xs.insert(0, 5)
    println(f"{xs.length()} {xs[0] ?? -1} {xs[1] == None} {xs[2] ?? -1}")
    xs[0] = None
    println(f"{xs[0] == None}")
    var ps = List<P?>()
    ps.push(None)
    ps.insert(0, None)
    ps.insert(0, P(x: 4))
    println(f"{ps.length()} {(ps[0] ?? P(x: -1)).x} {ps[1] == None}")
    var a [int?; 3] = [Some(1), Some(2), Some(3)]
    a[1] = None
    println(f"{a[1] == None} {a[2] ?? -1}")
"#,
        "3 5 true 7\ntrue\n3 4 true\ntrue 3",
    );
}

#[test]
fn test_a_trait_default_inherited_through_renamed_parameters_takes_absent_values() {
    assert_heap_guard_output(
        r#"
use system.io

trait Sink<T>
    fn put(x T?) int
    fn put2(x T?, y T?) int
        return self.put(x) + self.put(y)

class Bag<U> implements Sink<U>
    var n int
    fn put(y U?) int
        match y
            Some(v): return 1
            None: return 100

class Sub<V> extends Bag<V>
    var m int
    fn put2(a V?, b V?) int
        var t = 0
        match a
            Some(v): t = t + 3
            None: t = t + 300
        match b
            Some(v): t = t + 4
            None: t = t + 400
        return t

fn go(b Bag<int?>) int
    return b.put2(None, Some(None))

fn gop(b Bag<int>) int
    return b.put2(None, 5)

fn main()
    println(f"{go(Sub<int?>(n: 0, m: 0))} {gop(Sub<int>(n: 0, m: 0))}")
    println(f"{go(Bag<int?>(n: 0))} {gop(Bag<int>(n: 0))}")
"#,
        "304 304\n101 101",
    );
}

/// An optional field read out as a function's whole body, or assigned to an
/// optional one layer deeper, is wrapped once: the read is typed by the
/// field, not by the holder it is read from.
#[test]
fn test_an_optional_field_returned_or_assigned_is_wrapped_once() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    var v T?
    fn get() T?: self.v

struct S
    v int?

fn read(s S) Option<int?>: s.v

fn main()
    let b = Box<int>(v: 4)
    println(f"{b.get() ?? -1}")
    let s = S(v: None)
    match read(s)
        Some(inner): println(f"some {inner == None}")
        None: println("none")
    var r Option<int?> = None
    r = s.v
    match r
        Some(inner): println(f"some {inner == None}")
        None: println("none")
"#,
        "4\nsome true\nsome true",
    );
}

/// Inside `Sub<int>`, a `Sub<V?>` it builds reads its inherited field at its own
/// arguments through the `extends` chain: `int??`, not the body's own `int?`.
#[test]
fn test_a_derived_generic_class_builds_its_own_deeper_instance() {
    assert_heap_guard_output(
        r#"
use system.io

class Bag<U>
    var item U?
    var n int

class Sub<V> extends Bag<V>
    var m int
    fn lift() Sub<V?>
        return Sub<V?>(item: Some(None), n: 0, m: 0)
    fn lift_val(v V) Sub<V?>
        return Sub<V?>(item: v, n: 0, m: 0)

fn main()
    let s = Sub<int>(item: 3, n: 0, m: 0)
    let t = s.lift()
    match t.item
        Some(inner): println(f"some {inner == None}")
        None: println("none")
    let u = s.lift_val(8)
    match u.item
        Some(inner): println(f"some {inner ?? -1}")
        None: println("none")
"#,
        "some true
some 8",
    );
}

/// A `V?` argument stored into a `Sub<V?>` built inside `Sub<V>` is one layer short
/// of the inherited field, and is wrapped.
#[test]
fn test_a_derived_generic_class_wraps_an_optional_argument_into_its_deeper_instance() {
    assert_heap_guard_output(
        r#"
use system.io

class Bag<U>
    var item U?
    var n int

class Sub<V> extends Bag<V>
    var m int
    fn wrap(v V?) Sub<V?>
        return Sub<V?>(item: v, n: 0, m: 0)
    fn none_inner() Sub<V?>
        let x V? = None
        return Sub<V?>(item: x, n: 0, m: 0)

fn show(o Option<int?>) String
    match o
        Some(inner): return f"some({inner ?? -1})"
        None: return "none"

fn main()
    let s = Sub<int>(item: 3, n: 0, m: 0)
    println(f"{show(s.wrap(5).item)} {show(s.wrap(None).item)} {show(s.none_inner().item)}")
"#,
        "some(5) some(-1) some(-1)",
    );
}

/// A trait default handing its bare `T` to a `T?` parameter: the open template body
/// the class also emits has no settled layout for either, and is left alone.
#[test]
fn test_a_trait_default_passing_its_parameter_to_an_optional_parameter() {
    assert_heap_guard_output(
        r#"
use system.io

trait Store<T>
    fn put(x T?) int
    fn send(v T) int
        return self.put(v)

class Cell<T> implements Store<T>
    var n int
    fn put(y T?) int
        match y
            Some(v): return 1
            None: return 100

fn main()
    var c = Cell<int>(n: 0)
    println(f"{c.send(5)}")
"#,
        "1",
    );
}

/// The same through a class extending the implementing class, with renamed
/// parameters.
#[test]
fn test_an_inherited_trait_default_passing_its_parameter_to_an_optional_parameter() {
    assert_heap_guard_output(
        r#"
use system.io

trait Store<T>
    fn put(x T?) int
    fn send(v T) int
        return self.put(v)

class Cell<U> implements Store<U>
    var n int
    fn put(y U?) int
        match y
            Some(v): return 1
            None: return 100

class Sub<W> extends Cell<W>
    var z int

fn main()
    var c = Sub<int>(n: 0, z: 0)
    println(f"{c.send(5)}")
"#,
        "1",
    );
}

/// A class with a value parameter extending a generic base reads the base's
/// optional field at its own type argument, passed through the `extends`
/// clause: built outside and inside its methods, at a counted and an inline
/// payload, two instances side by side.
#[test]
fn test_a_value_generic_class_reads_its_inherited_optional_field_at_its_argument() {
    assert_heap_guard_output(
        r#"
use system.io

class Base<U>
    var item U?
    var n int

class Buf<V, Size> extends Base<V>
    var m int
    fn make_s(s String) String
        let b = Buf<String, Size>(item: s, n: 0, m: 0)
        let x = b.item
        let y = b.item
        return (x ?? "-") + (y ?? "-")
    fn make_i() int
        let b = Buf<int?, Size>(item: Some(None), n: 0, m: 0)
        match b.item
            Some(inner): return inner ?? 9
            None: return -1

fn main()
    let a = Buf<int, 2>(item: 1, n: 0, m: 0)
    let c = Buf<String, 2>(item: "q" + "r", n: 0, m: 0)
    let d = Buf<int?, 3>(item: None, n: 0, m: 0)
    println(f"{a.item ?? -1} {c.item ?? '-'} {d.item == None}")
    println(f"{c.make_s('s' + 't')} {a.make_i()}")
"#,
        "1 qr true\nstst 9",
    );
}

/// A field declared in a base two `extends` clauses up, each renaming the
/// parameter, is read at the argument the leaf binds.
#[test]
fn test_an_optional_field_inherited_through_two_renaming_clauses() {
    assert_heap_guard_output(
        r#"
use system.io

class Root<A>
    var item A?

class Mid<B> extends Root<B>
    var k int

class Leaf<C, Size> extends Mid<C>
    var m int
    fn deeper() Leaf<C?, Size>
        return Leaf<C?, Size>(item: Some(None), k: 0, m: 0)

fn main()
    let l = Leaf<int, 4>(item: 5, k: 1, m: 2)
    let s = Leaf<String, 4>(item: "x" + "y", k: 1, m: 2)
    println(f"{l.item ?? -1} {s.item ?? '-'}")
    let d = l.deeper()
    match d.item
        Some(inner): println(f"some {inner == None}")
        None: println("none")
"#,
        "5 xy\nsome true",
    );
}
