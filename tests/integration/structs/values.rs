// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A struct or tuple of scalars is held as its bytes: in a frame, inline in
//! the class, struct, tuple, collection, payload or closure holding it, and
//! copied whole wherever it is assigned or passed. Each program below runs
//! under the heap guard, so a stack address that outlived its frame, or a
//! release of bytes that were never an allocation, stops it.

use super::utils::*;

const POINT: &str = "
use system.io

struct P
    x float
    y float
";

fn with_point(body: &str) -> String {
    format!("{POINT}{body}")
}

/// Runs `code` under the heap guard expecting `expected`, then counts its
/// allocations: a value falling back to an allocation of its own changes the
/// count.
fn assert_runs_allocating(code: &str, expected: &str, allocations: usize) {
    assert_heap_guard_output(code, expected);
    assert_allocation_count(code, allocations);
}

#[test]
fn test_accumulating_a_struct_in_a_loop_allocates_nothing() {
    let code = with_point(
        "
fn add(a P, b P) P
    P(x: a.x + b.x, y: a.y + b.y)

fn main()
    var acc = P(x: 0.0, y: 0.0)
    var i = 0
    while i < 1000
        acc = add(acc, P(x: 1.0, y: 2.0))
        i += 1
    if acc.x == 1000.0 and acc.y == 2000.0
        println(\"summed\")
",
    );
    assert_heap_guard_output(&code, "summed");
    assert_allocation_count(&code, 0);
}

#[test]
fn test_an_element_bound_from_a_list_is_a_copy_of_it() {
    assert_runs_allocating(
        "
use system.io
use system.collections.list

struct Pt
    x float
    y float

fn main()
    var xs = List<Pt>()
    xs.push(Pt(x: 1.0, y: 2.0))
    var e = xs[0]
    e.x = 99.0
    let kept = xs[0]
    println(f\"{kept.x} {e.x}\")
",
        "1.0 99.0",
        5,
    );
}

const RAY_TRACER: &str = "
use system.io

struct Vec3
    x float
    y float
    z float

struct Ray
    origin Vec3
    dir Vec3

struct Sphere
    center Vec3
    radius float

fn sub(a Vec3, b Vec3) Vec3: Vec3(x: a.x - b.x, y: a.y - b.y, z: a.z - b.z)

fn dot(a Vec3, b Vec3) float: a.x * b.x + a.y * b.y + a.z * b.z

fn hits(s Sphere, r Ray) bool
    let oc = sub(r.origin, s.center)
    let b = dot(oc, r.dir)
    let c = dot(oc, oc) - s.radius * s.radius
    return b * b - c > 0.0

fn render(n int) int
    let sphere = Sphere(center: Vec3(x: 0.0, y: 0.0, z: 3.0), radius: 1.0)
    var lit = 0
    var y = 0
    while y < n
        var x = 0
        while x < n
            let u = (x as float) / (n as float) - 0.5
            let v = (y as float) / (n as float) - 0.5
            let ray = Ray(origin: Vec3(x: 0.0, y: 0.0, z: 0.0), dir: Vec3(x: u, y: v, z: 1.0))
            if hits(sphere, ray)
                lit += 1
            x += 1
        y += 1
    return lit

fn main()
    let lit = render(SIZE)
    println(f\"{lit}\")
";

#[test]
fn test_a_ray_tracer_allocates_the_same_at_any_image_size() {
    let small = RAY_TRACER.replace("SIZE", "8");
    let large = RAY_TRACER.replace("SIZE", "16");
    assert_heap_guard_output(&small, "64");
    assert_heap_guard_output(&large, "256");
    assert_eq!(
        get_allocation_count_value(&small),
        get_allocation_count_value(&large)
    );
}

#[test]
fn test_a_list_of_generic_pairs_allocates_its_buffers_not_its_elements() {
    let code = "
use system.io
use system.collections.list

struct Pair<A, B>
    first A
    second B

fn main()
    var xs = List<Pair<int, float>>()
    var i = 0
    while i < 1000
        xs.push(Pair(first: i, second: (i as float) * 0.5))
        i += 1
    let last = xs[999]
    if xs.length() == 1000 and last.first == 999 and last.second == 499.5
        println(\"filled\")
";
    assert_heap_guard_output(code, "filled");
    assert_allocation_count(code, 10);
}

#[test]
fn test_a_set_of_float_structs_matches_elements_as_equality_does() {
    assert_heap_guard_output(
        &with_point(
            "
use system.collections.set

fn main()
    let zero = 0.0
    let nan = zero / zero
    var s = Set<P>()
    s.add(P(x: -0.0, y: 1.0))
    s.add(P(x: 0.0, y: 1.0))
    println(f\"{s.length()} {s.contains(P(x: 0.0, y: 1.0))}\")
    let a = P(x: nan, y: 1.0)
    s.add(a)
    s.add(a)
    println(f\"{s.length()} {a == a} {s.contains(a)}\")
",
        ),
        "1 true\n3 false false",
    );
}

#[test]
fn test_a_value_in_a_class_field_is_written_in_place_and_copied_out() {
    assert_heap_guard_output(
        &with_point(
            "
class Body
    var pos P
    var mass float

fn main()
    var b = Body(pos: P(x: 1.0, y: 2.0), mass: 3.0)
    b.pos.x = 5.0
    let q = b.pos
    b.pos = P(x: 7.0, y: 8.0)
    println(f\"{q.x} {b.pos.x} {b.pos.y} {b.mass}\")
",
        ),
        "5.0 7.0 8.0 3.0",
    );
}

#[test]
fn test_a_value_in_a_tuple_and_a_nested_struct_is_its_own_copy() {
    assert_heap_guard_output(
        &with_point(
            "
struct Segment
    head P
    tail P

fn main()
    var s = Segment(head: P(x: 1.0, y: 2.0), tail: P(x: 3.0, y: 4.0))
    var t = (s.tail, 5)
    var c = s
    c.head.x = 9.0
    let moved = t.0
    println(f\"{s.head.x} {c.head.x} {s.tail.y} {moved.y} {t.1}\")
",
        ),
        "1.0 9.0 4.0 4.0 5",
    );
}

#[test]
fn test_a_value_captured_or_returned_by_a_closure_outlives_its_frame() {
    assert_runs_allocating(
        &with_point(
            "
fn mirror_of(p P) fn() P
    let flip = fn() P: P(x: p.y, y: p.x)
    return flip

fn main()
    var p = P(x: 1.0, y: 2.0)
    let get = mirror_of(p)
    p.x = 50.0
    let m = get()
    println(f\"{m.x} {m.y} {p.x}\")
",
        ),
        "2.0 1.0 50.0",
        5,
    );
}

#[test]
fn test_a_value_held_in_an_enum_payload_outlives_its_frame() {
    assert_runs_allocating(
        &with_point(
            "
enum Shape
    Dot(P)
    Line(P, P)

fn line() Shape
    let a = P(x: 1.0, y: 2.0)
    return Shape.Line(a, P(x: 3.0, y: 4.0))

fn main()
    let s = line()
    match s
        Shape.Dot(p): println(f\"dot {p.x}\")
        Shape.Line(a, b): println(f\"line {a.x} {b.y}\")
",
        ),
        "line 1.0 4.0",
        4,
    );
}

#[test]
fn test_a_value_held_in_a_map_outlives_its_frame() {
    assert_runs_allocating(
        &with_point(
            "
use system.collections.map

fn table() Map<String, P>
    var m = Map<String, P>()
    m[\"a\"] = P(x: 1.0, y: 2.0)
    m[\"b\"] = P(x: 3.0, y: 4.0)
    return m

fn main()
    var m = table()
    m[\"a\"].x = 10.0
    let a = m[\"a\"]
    let b = m[\"b\"]
    println(f\"{a.x} {b.y}\")
",
        ),
        "10.0 4.0",
        7,
    );
}

#[test]
fn test_a_value_crosses_a_direct_call() {
    assert_heap_guard_output(
        &with_point(
            "
fn swap(p P) P: P(x: p.y, y: p.x)

fn main()
    let s = swap(swap(P(x: 1.0, y: 2.0)))
    println(f\"{s.x} {s.y}\")
",
        ),
        "1.0 2.0",
    );
}

#[test]
fn test_a_value_crosses_a_virtual_call() {
    assert_heap_guard_output(
        &with_point(
            "
trait Mover
    fn moved(p P) P

class Shift implements Mover
    var dx float
    fn moved(p P) P: P(x: p.x + self.dx, y: p.y)

fn main()
    let m Mover = Shift(dx: 10.0)
    let r = m.moved(P(x: 1.0, y: 2.0))
    println(f\"{r.x} {r.y}\")
",
        ),
        "11.0 2.0",
    );
}

#[test]
fn test_a_value_crosses_a_closure_call() {
    assert_heap_guard_output(
        &with_point(
            "
fn main()
    let k = 3.0
    let scale = fn(p P) P: P(x: p.x * k, y: p.y * k)
    let r = scale(P(x: 1.0, y: 2.0))
    println(f\"{r.x} {r.y}\")
",
        ),
        "3.0 6.0",
    );
}

#[test]
fn test_a_value_crosses_a_call_through_a_function_value() {
    assert_heap_guard_output(
        &with_point(
            "
fn swap(p P) P: P(x: p.y, y: p.x)

fn apply(f fn(p P) P, p P) P: f(p)

fn main()
    let r = apply(swap, P(x: 1.0, y: 2.0))
    println(f\"{r.x} {r.y}\")
",
        ),
        "2.0 1.0",
    );
}

#[test]
fn test_a_value_passes_through_generic_code_at_its_own_type() {
    assert_heap_guard_output(
        &with_point(
            "
struct Box<T>
    item T

fn id<T>(x T) T: x

fn main()
    let q = P(x: 1.0, y: 2.0)
    var b = Box(item: q)
    b.item.x = 4.0
    let same = id(q)
    println(f\"{b.item.x} {q.x} {same.y}\")
",
        ),
        "4.0 1.0 2.0",
    );
}

#[test]
fn test_a_mixed_width_value_keeps_every_field_in_a_list_and_a_class() {
    assert_heap_guard_output(
        "
use system.io
use system.collections.list

struct Mixed
    small i8
    whole int
    single f32

class Holder
    var value Mixed

fn main()
    var xs = List<Mixed>()
    xs.push(Mixed(small: -3, whole: 70000, single: 1.5))
    xs.push(Mixed(small: 7, whole: -2, single: 0.25))
    let h = Holder(value: xs[1])
    let first = xs[0]
    println(f\"{first.small} {first.whole} {first.single}\")
    println(f\"{h.value.small} {h.value.whole} {h.value.single}\")
",
        "-3 70000 1.5\n7 -2 0.25",
    );
}

#[test]
fn test_a_value_out_parameter_is_written_in_the_callers_bytes() {
    assert_heap_guard_output(
        &with_point(
            "
fn reset(p out P)
    p = P(x: 0.5, y: 0.25)

fn bump(p out P)
    p.x = p.x + 1.0

fn main()
    var q = P(x: 9.0, y: 9.0)
    reset(q)
    bump(q)
    println(f\"{q.x} {q.y}\")
",
        ),
        "1.5 0.25",
    );
}

#[test]
fn test_a_value_crosses_a_static_and_an_async_call() {
    assert_heap_guard_output(
        &with_point(
            "
class Registry
    public static fn origin(scale float) P
        return P(x: scale, y: scale * 2.0)

async fn swapped(p P) P
    return P(x: p.y, y: p.x)

fn main()
    let o = Registry.origin(3.0)
    let s = swapped(o)
    println(f\"{o.x} {o.y} {s.x} {s.y}\")
",
        ),
        "3.0 6.0 6.0 3.0",
    );
}

#[test]
fn test_a_value_read_through_a_field_is_stored_whole_in_a_collection() {
    assert_heap_guard_output(
        &with_point(
            "
use system.collections.list
use system.collections.set
use system.collections.map

class Body
    var pos P

fn make(v float) P: P(x: v, y: v + 1.0)

fn main()
    var b = Body(pos: P(x: 1.0, y: 2.0))
    var xs = List<P>()
    xs.push(b.pos)
    let t = (P(x: 5.0, y: 6.0), 1)
    xs.push(t.0)
    var s = Set<P>()
    s.add(b.pos)
    var m = Map<int, P>()
    m[1] = t.0
    b.pos = make(7.0)
    xs[0] = b.pos
    let popped = xs.pop()
    let first = xs[0]
    let got = m[1]
    println(f\"{first.x} {first.y} {xs.length()} {s.contains(P(x: 1.0, y: 2.0))} {got.y}\")
",
        ),
        "7.0 8.0 1 true 6.0",
    );
}

const TOKEN: &str = "
use system.io
use system.collections.list

enum Kind
    Small
    Big

struct Token
    kind Kind
    pos int
    width int?
";

fn with_token(body: &str) -> String {
    format!("{TOKEN}{body}")
}

#[test]
fn test_a_struct_held_as_a_reference_is_copied_when_a_binding_is_written() {
    assert_heap_guard_output(
        &with_token(
            "
fn main()
    var a = Token(kind: Kind.Small, pos: 1, width: 3)
    var b = a
    b.pos = 5
    b.width = None
    let w = a.width ?? -1
    let v = b.width ?? -1
    println(f\"{a.pos} {b.pos} {w} {v}\")
",
        ),
        "1 5 3 -1",
    );
}

#[test]
fn test_a_struct_held_as_a_reference_captured_by_a_closure_keeps_its_value() {
    assert_heap_guard_output(
        &with_token(
            "
fn main()
    var a = Token(kind: Kind.Small, pos: 1, width: 3)
    let f = fn() int: a.pos
    a.pos = 9
    println(f\"{f()} {a.pos}\")
",
        ),
        "1 9",
    );
}

#[test]
fn test_a_struct_held_as_a_reference_in_a_copied_list_is_written_only_in_one() {
    assert_heap_guard_output(
        &with_token(
            "
fn main()
    let a = Token(kind: Kind.Small, pos: 9, width: 3)
    var xs = List([a])
    var ys = xs
    ys[0].pos = 100
    let first = xs[0]
    let other = ys[0]
    println(f\"{first.pos} {other.pos}\")
",
        ),
        "9 100",
    );
}

#[test]
fn test_an_optional_value_is_compared_opened_and_stored() {
    assert_runs_allocating(
        &with_point(
            "
use system.collections.list

fn find(k int) P?
    if k == 1
        return P(x: 1.0, y: 2.0)
    return None

fn main()
    let missing = find(2)
    let present = find(1)
    let fallback = missing ?? P(x: 9.0, y: 9.0)
    let kept = present ?? P(x: 9.0, y: 9.0)
    var xs = List<P?>()
    xs.push(present)
    xs.push(None)
    xs.push(P(x: 3.0, y: 4.0))
    var count = 0
    for o in xs
        if let Some(p) = o
            count += 1
    let last = xs[2] ?? P(x: 0.0, y: 0.0)
    println(f\"{missing == None} {present == None} {present != None} {fallback.x} {kept.y} {count} {last.y}\")
",
        ),
        "true false true 9.0 2.0 2 4.0",
        // A `P?` is held as its bytes like `P`: no optional is boxed.
        10,
    );
}

#[test]
fn test_a_map_keyed_by_values_matches_keys_as_equality_does() {
    assert_heap_guard_output(
        &with_point(
            "
use system.collections.map

fn main()
    var m = Map<P, int>()
    m[P(x: -0.0, y: 1.0)] = 1
    m[P(x: 0.0, y: 1.0)] = 2
    var t = Map<(int, int), P>()
    t[(1, 2)] = P(x: 5.0, y: 6.0)
    let hit = t.get((1, 2))
    var found = 0.0
    if let Some(p) = hit
        found = p.y
    let missing = t.get((2, 1))
    let both = m[P(x: 0.0, y: 1.0)]
    println(f\"{m.length()} {both} {found} {missing == None}\")
",
        ),
        "1 2 6.0 true",
    );
}

#[test]
fn test_a_value_returned_from_every_arm_of_a_match_on_an_int() {
    assert_heap_guard_output(
        "
use system.io

struct P
    x int
    y int

fn pick(n int) P
    match n
        0: return P(x: -1, y: -1)
        _: return P(x: 1, y: 1)

fn main()
    println(f\"{pick(0).x} {pick(1).x}\")
",
        "-1 1",
    );
}

#[test]
fn test_a_value_returned_from_every_arm_of_a_match_on_an_enum() {
    assert_heap_guard_output(
        "
use system.io

struct P
    x int
    y int

enum K
    A
    B

fn of(k K) P
    match k
        K.A: return P(x: 1, y: 1)
        K.B: return P(x: 2, y: 2)

fn main()
    println(f\"{of(K.B).x}\")
",
        "2",
    );
}

#[test]
fn test_a_value_returned_from_every_arm_of_a_match_on_an_optional() {
    assert_heap_guard_output(
        "
use system.io

struct P
    x int
    y int

fn of(o P?) P
    match o
        Some(p): return p
        None: return P(x: 0, y: 0)

fn main()
    println(f\"{of(P(x: 3, y: 3)).x}\")
",
        "3",
    );
}

#[test]
fn test_a_tuple_returned_from_every_arm_after_an_early_return() {
    assert_heap_guard_output(
        "
use system.io

fn of(n int) (int, int)
    if n == 0
        return (0, 0)
    match n
        1: return (1, 1)
        _: return (2, 2)

fn main()
    let t = of(1)
    println(f\"{t.0}\")
",
        "1",
    );
}

#[test]
fn test_a_value_argument_read_through_a_field_is_a_copy_the_callee_keeps() {
    assert_heap_guard_output(
        &with_point(
            "
class C
    var p P
    fn set()
        self.p = P(x: 5.0, y: 5.0)

fn f(c C, q P) float
    c.set()
    q.x

fn main()
    let c = C(p: P(x: 1.0, y: 2.0))
    println(f\"{f(c, c.p)}\")
",
        ),
        "1.0",
    );
}

#[test]
fn test_a_value_argument_outlives_the_holder_the_callee_replaces() {
    assert_heap_guard_output(
        &with_point(
            "
class C
    var p P

class Holder
    var c C
    fn replace()
        self.c = C(p: P(x: 5.0, y: 5.0))
        var i = 0
        while i < 50
            let junk = C(p: P(x: 77.0, y: 77.0))
            i += 1

fn f(h Holder, q P) float
    h.replace()
    q.x

fn main()
    let h = Holder(c: C(p: P(x: 1.0, y: 2.0)))
    println(f\"{f(h, h.c.p)}\")
",
        ),
        "1.0",
    );
}

#[test]
fn test_a_value_argument_read_from_the_receiver_is_a_copy() {
    assert_heap_guard_output(
        &with_point(
            "
class C
    var p P
    fn take(q P) float
        self.p = P(x: 5.0, y: 5.0)
        return q.x

fn main()
    let c = C(p: P(x: 1.0, y: 2.0))
    let before = c.take(c.p)
    println(f\"{before} {c.p.x}\")
",
        ),
        "1.0 5.0",
    );
}

const SIXTEEN_WORDS: &str = "
use system.io

struct S
    f0 i64
    f1 i64
    f2 i64
    f3 i64
    f4 i64
    f5 i64
    f6 i64
    f7 i64
    f8 i64
    f9 i64
    f10 i64
    f11 i64
    f12 i64
    f13 i64
    f14 i64
    f15 i64
EXTRA
fn bump(s S) S
    var t = s
    t.f0 = t.f0 + 1
    return t

fn main()
    var a = S(f0: 0, f1: 0, f2: 0, f3: 0, f4: 0, f5: 0, f6: 0, f7: 0, f8: 0, f9: 0, f10: 0, f11: 0, f12: 0, f13: 0, f14: 0, f15: 0INIT)
    var i = 0
    while i < 100
        a = bump(a)
        i += 1
    if a.f0 == 100
        println(\"bumped\")
";

#[test]
fn test_a_struct_of_exactly_128_bytes_is_held_as_its_bytes() {
    let code = SIXTEEN_WORDS.replace("EXTRA", "").replace("INIT", "");
    assert_runs_allocating(&code, "bumped", 0);
}

#[test]
fn test_a_struct_past_128_bytes_is_a_counted_allocation() {
    let code = SIXTEEN_WORDS
        .replace("EXTRA", "    extra i8")
        .replace("INIT", ", extra: 1");
    assert_runs_allocating(&code, "bumped", 1);
}

#[test]
fn test_a_value_with_128_bit_fields_round_trips_through_a_list() {
    assert_heap_guard_output(
        "
use system.io
use system.collections.list

struct Wide
    tag i8
    big i128
    small u128

fn main()
    let w = Wide(tag: 3, big: -170141183460469231731687303715884105728, small: 340282366920938463463374607431768211455)
    var ws = List<Wide>()
    ws.push(w)
    let back = ws[0]
    println(f\"{back.tag} {back.big == w.big} {back.small == w.small}\")
",
        "3 true true",
    );
}

#[test]
fn test_an_array_of_values_is_written_and_read_in_place() {
    assert_heap_guard_output(
        &with_point(
            "
fn main()
    var arr = [P(x: 1.0, y: 2.0), P(x: 3.0, y: 4.0), P(x: 5.0, y: 6.0), P(x: 7.0, y: 8.0)]
    arr[2].x = 50.0
    let third = arr[2]
    let last = arr[3]
    println(f\"{third.x} {last.y} {arr.length()}\")
",
        ),
        "50.0 8.0 4",
    );
}

#[test]
fn test_a_tuple_inside_a_struct_inside_a_list_is_written_in_place() {
    assert_heap_guard_output(
        &with_point(
            "
use system.collections.list

struct Holder
    name String
    pair (int, P)

fn main()
    var hs = List<Holder>()
    hs.push(Holder(name: \"a\", pair: (1, P(x: 9.0, y: 10.0))))
    hs[0].pair.1.y = 11.0
    let h = hs[0]
    let inner = h.pair.1
    println(f\"{h.name} {h.pair.0} {inner.y}\")
",
        ),
        "a 1 11.0",
    );
}

#[test]
fn test_two_value_elements_swap_through_a_binding() {
    assert_heap_guard_output(
        &with_point(
            "
use system.collections.list

fn main()
    var xs = List([P(x: 1.0, y: 1.0), P(x: 2.0, y: 2.0)])
    let tmp = xs[0]
    xs[0] = xs[1]
    xs[1] = tmp
    let first = xs[0]
    let second = xs[1]
    println(f\"{first.x} {second.x}\")
",
        ),
        "2.0 1.0",
    );
}
