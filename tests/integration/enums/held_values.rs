// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An enum whose variants carry only scalars and values held as their bytes —
//! a payload-free enum included — is itself held as its bytes: its tag and the
//! payload columns beside it, in a frame, inline in the struct holding it or
//! in a collection's buffer. Each program below runs under the heap guard, so
//! a stack address that outlived its frame, or a release of bytes that were
//! never an allocation, stops it; the allocation counts prove no value fell
//! back to an allocation of its own.

use super::utils::*;

const SHAPES: &str = "
use system.io

enum Dir
    North
    East
    South
    West

enum Shape
    Circle(float)
    Rect(float, float)

fn area(s Shape) float
    match s
        Shape.Circle(r): r * r * 3.0
        Shape.Rect(w, h): w * h

fn turn(d Dir) Dir
    match d
        Dir.North: Dir.East
        Dir.East: Dir.South
        Dir.South: Dir.West
        Dir.West: Dir.North
";

fn with_shapes(body: &str) -> String {
    format!("{SHAPES}{body}")
}

/// What two lists grown to 1000 elements allocate: the lists themselves and
/// each buffer they grow into, the same for a list of `int`.
const LIST_BUFFER_ALLOCATIONS: usize = 20;

/// Runs `code` under the heap guard expecting `expected`, then counts its
/// allocations: an enum value falling back to an allocation of its own
/// changes the count.
fn assert_runs_allocating(code: &str, expected: &str, allocations: usize) {
    assert_heap_guard_output(code, expected);
    assert_allocation_count(code, allocations);
}

#[test]
fn test_a_payload_free_enum_local_allocates_nothing() {
    assert_runs_allocating(
        &with_shapes(
            "
fn main()
    var d = Dir.North
    var i = 0
    while i < 1000
        d = turn(d)
        i += 1
    if d == Dir.North
        println(\"turned\")
",
        ),
        "turned",
        0,
    );
}

#[test]
fn test_an_enum_of_floats_local_allocates_nothing() {
    assert_runs_allocating(
        &with_shapes(
            "
fn main()
    var total = 0.0
    var i = 0
    while i < 1000
        let s = if i % 2 == 0: Shape.Circle(1.0) else: Shape.Rect(2.0, 3.0)
        total += area(s)
        i += 1
    if total == 4500.0
        println(\"summed\")
",
        ),
        "summed",
        0,
    );
}

#[test]
fn test_enums_in_struct_fields_allocate_nothing() {
    assert_runs_allocating(
        &with_shapes(
            "
struct Piece
    facing Dir
    shape Shape

fn reshape(s Shape) Shape
    match s
        Shape.Circle(r): Shape.Rect(r, 6.0)
        Shape.Rect(w, _): Shape.Circle(w)

fn main()
    var p = Piece(facing: Dir.North, shape: Shape.Circle(2.0))
    var i = 0
    while i < 1000
        p = Piece(facing: turn(p.facing), shape: reshape(p.shape))
        i += 1
    if p.facing == Dir.North and area(p.shape) == 12.0
        println(\"placed\")
",
        ),
        "placed",
        0,
    );
}

#[test]
fn test_enums_in_a_list_allocate_only_the_buffers() {
    assert_runs_allocating(
        &with_shapes(
            "
use system.collections.list

fn main()
    var shapes = List<Shape>()
    var dirs = List<Dir>()
    var i = 0
    while i < 1000
        shapes.push(Shape.Rect(1.0, i as float))
        dirs.push(turn(Dir.West))
        i += 1
    var total = 0.0
    for s in shapes
        total += area(s)
    if total == 499500.0 and dirs[999] == Dir.North and shapes.length() == 1000
        println(\"listed\")
",
        ),
        "listed",
        LIST_BUFFER_ALLOCATIONS,
    );
}

const TOKENS: &str = "
use system.io

struct P
    x int
    y float

enum Token
    Num(int)
    Real(float)
    Pair(i8, P)

fn describe(t Token) String
    match t
        Token.Num(n): f\"num {n}\"
        Token.Real(r): f\"real {r}\"
        Token.Pair(b, p): f\"pair {b} {p.x} {p.y}\"
";

fn with_tokens(body: &str) -> String {
    format!("{TOKENS}{body}")
}

#[test]
fn test_a_held_enum_is_built_matched_and_copied_whole() {
    assert_heap_guard_output(
        &with_shapes(
            "
fn main()
    var a = Shape.Rect(2.0, 3.0)
    var b = a
    b = Shape.Circle(1.0)
    var d = Dir.South
    let e = d
    d = turn(d)
    println(f\"{area(a)} {area(b)} {e == Dir.South} {d == Dir.West}\")
",
        ),
        "6.0 3.0 true true",
    );
}

/// A column whose variants store an `int`, a `float` and an `i8` is read
/// back as the member the arm binds, and a struct payload beside it keeps
/// both its fields.
#[test]
fn test_an_enum_of_mixed_payloads_reads_each_variant_back() {
    assert_heap_guard_output(
        &with_tokens(
            "
fn main()
    let tokens = [Token.Num(-7), Token.Real(2.5), Token.Pair(3, P(x: 40, y: 0.5))]
    for t in tokens
        println(describe(t))
",
        ),
        "num -7\nreal 2.5\npair 3 40 0.5",
    );
}

#[test]
fn test_a_held_enum_in_a_struct_field_is_written_and_read_in_place() {
    assert_heap_guard_output(
        &with_tokens(
            "
struct Slot
    id int
    token Token

fn main()
    var s = Slot(id: 1, token: Token.Real(1.5))
    s.token = Token.Pair(2, P(x: 9, y: 4.5))
    let copy = s
    s.token = Token.Num(5)
    println(f\"{describe(copy.token)} | {describe(s.token)}\")
",
        ),
        "pair 2 9 4.5 | num 5",
    );
}

#[test]
fn test_a_list_of_held_enums_pushes_pops_indexes_and_iterates() {
    assert_heap_guard_output(
        &with_tokens(
            "
use system.collections.list

fn main()
    var xs = List<Token>()
    xs.push(Token.Num(1))
    xs.push(Token.Real(2.0))
    xs.push(Token.Pair(3, P(x: 4, y: 5.0)))
    xs.push(Token.Num(6))
    let last = xs.pop()
    xs[0] = Token.Real(0.5)
    println(describe(xs[2]))
    match last
        Some(t): println(describe(t))
        None: println(\"empty\")
    for t in xs
        println(describe(t))
",
        ),
        "pair 3 4 5.0\nnum 6\nreal 0.5\nreal 2.0\npair 3 4 5.0",
    );
}

#[test]
fn test_an_optional_held_enum_is_present_absent_and_compared() {
    assert_heap_guard_output(
        &with_shapes(
            "
fn pick(wide bool) Shape?
    if wide: Shape.Rect(4.0, 1.0) else: None

fn main()
    let some = pick(true)
    let none = pick(false)
    let also Option<Shape> = Some(Shape.Rect(4.0, 1.0))
    match some
        Some(s): println(f\"some {area(s)}\")
        None: println(\"none\")
    println(f\"{none == None} {some == also} {some == none}\")
",
        ),
        "some 4.0\ntrue true false",
    );
}

#[test]
fn test_a_set_and_a_map_keyed_by_held_enums_match_keys_as_equality_does() {
    assert_heap_guard_output(
        &with_shapes(
            "
use system.collections.set
use system.collections.map

fn main()
    var dirs = Set<Dir>()
    dirs.add(Dir.North)
    dirs.add(turn(Dir.West))
    dirs.add(Dir.East)
    var shapes = Set<Shape>()
    shapes.add(Shape.Circle(1.0))
    shapes.add(Shape.Circle(1.0))
    shapes.add(Shape.Rect(1.0, 2.0))
    shapes.add(Shape.Circle(-0.0))
    shapes.add(Shape.Circle(0.0))
    var steps = Map<Dir, int>()
    steps[Dir.North] = 1
    steps[Dir.South] = 2
    steps[turn(Dir.West)] = 3
    println(f\"{dirs.length()} {shapes.length()} {steps.length()} {steps[Dir.North]}\")
    println(f\"{shapes.contains(Shape.Rect(1.0, 2.0))} {shapes.contains(Shape.Rect(2.0, 1.0))}\")
",
        ),
        "2 3 2 3\ntrue false",
    );
}

/// A tuple holding an enum is compared and hashed by the fields of the
/// variant its tag names, never by the column bytes another variant left.
#[test]
fn test_a_tuple_holding_a_held_enum_compares_and_hashes_by_variant() {
    assert_heap_guard_output(
        &with_tokens(
            "
use system.collections.set

fn main()
    let a = (1, Token.Real(2.0))
    let b = (1, Token.Real(2.0))
    let c = (1, Token.Num(2))
    var seen = Set<(int, Token)>()
    seen.add(a)
    seen.add(b)
    seen.add(c)
    seen.add((1, Token.Pair(1, P(x: 2, y: 3.0))))
    seen.add((1, Token.Pair(1, P(x: 2, y: 3.0))))
    println(f\"{a == b} {a == c} {seen.length()}\")
",
        ),
        "true false 3",
    );
}

#[test]
fn test_a_held_enum_renders_in_a_formatted_string() {
    assert_heap_guard_output(
        &with_shapes(
            "
fn main()
    let d = Dir.East
    let s = Shape.Rect(1.0, 2.0)
    println(f\"{d} {s}\")
",
        ),
        "East Rect(1.0, 2.0)",
    );
}

#[test]
fn test_a_method_on_a_held_enum_reads_its_receiver() {
    assert_heap_guard_output(
        "
use system.io

enum Shape
    Circle(float)
    Rect(float, float)

    fn area() float
        match self
            Shape.Circle(r): r * r * 3.0
            Shape.Rect(w, h): w * h

    fn bigger(other Shape) bool
        return self.area() > other.area()

fn main()
    let c = Shape.Circle(2.0)
    let r = Shape.Rect(2.0, 3.0)
    println(f\"{c.area()} {r.area()} {c.bigger(r)} {r.bigger(c)}\")
",
        "12.0 6.0 true false",
    );
}

/// A generic enum is reached through bodies its instantiations share, and
/// stays counted; it keeps working at a scalar and at a held struct.
#[test]
fn test_a_generic_enum_at_a_scalar_and_a_held_struct_keeps_its_values() {
    assert_heap_guard_output(
        "
use system.io

struct P
    x int
    y float

enum Either<T>
    One(T)
    Two(T, T)

    static fn pair(a T, b T) Self
        return Either.Two(a, b)

fn first<T>(e Either<T>) T
    match e
        Either.One(v): v
        Either.Two(v, _): v

fn main()
    let i Either<int> = Either.pair(3, 4)
    let p = Either.One(P(x: 5, y: 6.5))
    let q = first(p)
    println(f\"{first(i)} {q.x} {q.y}\")
",
        "3 5 6.5",
    );
}

/// An enum declaring a drop hook keeps its identity: it is a counted
/// allocation, and its hook runs once, when the last holder lets go.
#[test]
fn test_an_enum_with_a_drop_hook_stays_a_counted_allocation() {
    let code = "
use system.io

enum Handle
    Open(int)

    fn drop()
        match self
            Handle.Open(n): println(f\"closed {n}\")

fn main()
    let a = Handle.Open(1)
    let b = a
    match b
        Handle.Open(n): println(f\"held {n}\")
";
    assert_heap_guard_output(code, "held 1\nclosed 1");
    // The enum's own allocation, and the two strings its output formats.
    assert_allocation_count(code, 3);
}

/// An enum held inside another's column is matched by nested patterns, kept
/// in a class field, captured by a closure and stored as a map value.
#[test]
fn test_a_held_enum_nested_in_another_reaches_every_holder() {
    assert_heap_guard_output(
        "
use system.io
use system.collections.map

enum Inner
    A(int)
    B(float, int)

enum Outer
    Wrap(Inner)
    Pair(Inner, Inner)
    Empty

class Box
    var item Outer

    fn init(item Outer)
        self.item = item

fn sum(o Outer) float
    match o
        Outer.Wrap(Inner.A(n)): n as float
        Outer.Wrap(Inner.B(f, n)): f + (n as float)
        Outer.Pair(Inner.A(x), Inner.B(f, _)): (x as float) * f
        Outer.Pair(_, _): -1.0
        Outer.Empty: 0.0

fn make(k int) fn() Outer
    let captured = Outer.Pair(Inner.A(k), Inner.B(0.5, 1))
    return fn() Outer: captured

fn main()
    var b = Box(Outer.Wrap(Inner.B(1.5, 2)))
    let before = sum(b.item)
    b.item = Outer.Pair(Inner.A(4), Inner.B(2.5, 9))
    let maker = make(6)
    var m = Map<String, Outer>()
    m[\"w\"] = Outer.Wrap(Inner.A(7))
    m[\"e\"] = Outer.Empty
    let w = sum(m[\"w\"])
    let e = sum(m[\"e\"])
    println(f\"{before} {sum(b.item)} {sum(maker())} {w} {e}\")
",
        "3.5 10.0 3.0 7.0 0.0",
    );
}

/// Holding an enum as its bytes does not make it a device type: a device
/// binding of one is refused as it was when every enum was an allocation.
#[test]
fn test_a_device_binding_of_held_enums_is_refused() {
    assert_compiler_error(
        "
enum Instr
    Add(int, int)
    Neg(int)

gpu let code = [Instr.Add(1, 2), Instr.Neg(3)]
gpu var out = [0, 0]

forall i in 0..2
    out[i] = 1

let host = out
println(f'{host[0]}')
",
        "Type Not Accelerable",
    );
}

/// An enum that carries a string stays counted; the held enum beside the
/// string is copied into its payload, and releasing the counted enum frees
/// the string and nothing else.
#[test]
fn test_a_counted_enum_carries_a_held_enum_in_its_payload() {
    assert_heap_guard_output(
        &with_shapes(
            "
enum Labeled
    Named(String, Shape)
    Bare(Dir)

fn show(l Labeled) String
    match l
        Labeled.Named(name, s): f\"{name} {area(s)}\"
        Labeled.Bare(d): f\"bare {d == Dir.South}\"

fn main()
    var s = Shape.Rect(1.0, 2.0)
    let named = Labeled.Named(\"rect\", s)
    s = Shape.Circle(1.0)
    println(f\"{show(named)} {show(Labeled.Bare(Dir.South))} {area(s)}\")
",
        ),
        "rect 2.0 bare true 3.0",
    );
}

/// An enum writing its own `equals` decides alone what equal means, so it
/// stays counted: `==` on it, on a struct holding it, on a tuple holding it
/// and on an optional of it all reach that `equals`, which calls no `Null`
/// equal to anything.
#[test]
fn test_an_enum_with_its_own_equals_decides_every_comparison_holding_it() {
    assert_heap_guard_output(
        "
use system.io

enum Cell
    Null
    Num(int)

    fn equals(other Self) bool
        match self
            Cell.Null: false
            Cell.Num(n):
                match other
                    Cell.Num(m): n == m
                    Cell.Null: false

struct Row
    c Cell

fn pick(k int) Cell?
    if k > 0: Cell.Null else: None

fn main()
    let r1 = Row(c: Cell.Null)
    let r2 = Row(c: Cell.Null)
    let t1 = (Cell.Null, 1)
    let t2 = (Cell.Null, 1)
    println(f\"{Cell.Null == Cell.Null} {r1 == r2} {t1 == t2} {pick(1) == pick(2)}\")
    println(f\"{Cell.Num(3) == Cell.Num(3)} {Row(c: Cell.Num(4)) == Row(c: Cell.Num(4))}\")
",
        "false false false false\ntrue true",
    );
}

/// An enum too wide to come back in registers, and one whose columns mix
/// integer and float members, are returned, passed and written through an
/// `out` parameter whole.
#[test]
fn test_wide_and_mixed_held_enums_cross_calls_whole() {
    assert_heap_guard_output(
        "
use system.io

enum Big
    A(int, int, int, int, int)
    B(float, float, float)

enum Mix
    I(int, float)
    F(float, int)

fn make_big(k int) Big
    if k > 0: Big.A(k, k + 1, k + 2, k + 3, k + 4) else: Big.B(0.5, 1.5, 2.5)

fn total(b Big) float
    match b
        Big.A(a, b2, c, d, e): (a + b2 + c + d + e) as float
        Big.B(x, y, z): x + y + z

fn flip(m Mix) Mix
    match m
        Mix.I(i, f): Mix.F(f, i)
        Mix.F(f, i): Mix.I(i * 2, f * 2.0)

fn show(m Mix) String
    match m
        Mix.I(i, f): f\"I {i} {f}\"
        Mix.F(f, i): f\"F {f} {i}\"

fn grow(b out Big)
    match b
        Big.A(a, _, _, _, e): b = Big.A(a * 10, 0, 0, 0, e)
        Big.B(x, _, _): b = Big.B(x, x, x)

fn main()
    let a = make_big(1)
    let b = make_big(0)
    println(f\"{total(a)} {total(b)}\")
    var m = Mix.I(3, 0.25)
    m = flip(m)
    println(show(m))
    m = flip(m)
    println(show(m))
    var g = make_big(2)
    grow(g)
    var h = make_big(-1)
    grow(h)
    println(f\"{total(g)} {total(h)} {g == Big.A(20, 0, 0, 0, 6)} {h == Big.B(0.5, 0.5, 0.5)}\")
",
        "15.0 4.5\nF 0.25 3\nI 6 0.5\n26.0 1.5 true true",
    );
}

/// A value reached by overwriting a wider variant equals, hashes and keys
/// like the same variant built fresh: the bytes the earlier variant left in
/// a column are no part of it, wherever the value sits.
#[test]
fn test_a_variant_overwriting_another_equals_the_same_variant_built_fresh() {
    assert_heap_guard_output(
        &with_tokens(
            "
use system.collections.set
use system.collections.map
use system.collections.list

struct Slot
    id int
    token Token

fn main()
    var a = Token.Pair(7, P(x: 1, y: 2.0))
    a = Token.Num(5)
    let fresh = Token.Num(5)
    var seen = Set<Token>()
    seen.add(a)
    seen.add(fresh)
    var keyed = Map<Token, int>()
    keyed[a] = 1
    keyed[fresh] = 2
    println(f\"{a == fresh} {seen.length()} {keyed.length()} {keyed[a]} {(a, 1) == (fresh, 1)}\")
    var narrow = Token.Num(-1)
    narrow = Token.Pair(7, P(x: 1, y: 2.0))
    let pair = Token.Pair(7, P(x: 1, y: 2.0))
    seen.add(narrow)
    seen.add(pair)
    println(f\"{narrow == pair} {seen.length()} {(narrow, 2) == (pair, 2)}\")
    var xs = List<Token>()
    xs.push(Token.Pair(7, P(x: 1, y: 2.0)))
    xs[0] = Token.Num(5)
    var s = Slot(id: 1, token: Token.Pair(9, P(x: 3, y: 4.0)))
    s.token = Token.Num(5)
    println(f\"{xs[0] == fresh} {s.token == fresh} {s == Slot(id: 1, token: fresh)}\")
",
        ),
        "true 1 1 2 true\ntrue 2 true\ntrue true true",
    );
}

/// A circle overwriting a rectangle equals a circle built fresh, and a
/// circle's radius compares as `float ==` compares it: `-0.0` equals `0.0`.
#[test]
fn test_a_float_payload_compares_as_float_equality_does() {
    assert_heap_guard_output(
        &with_shapes(
            "
fn main()
    var sh = Shape.Rect(2.0, 3.0)
    sh = Shape.Circle(1.0)
    let zero = 0.0
    let negative = -zero
    println(f\"{sh == Shape.Circle(1.0)} {negative == zero} {Shape.Circle(negative) == Shape.Circle(zero)}\")
",
        ),
        "true true true",
    );
}

/// Held enums nest in every holder: an optional in a list, assigned absent
/// and present; an array; a tuple of a tuple passed and returned; a struct
/// inside a struct inside a variant, built, rebuilt, copied and compared.
#[test]
fn test_held_enums_nest_in_optionals_arrays_tuples_and_structs() {
    assert_heap_guard_output(
        &with_shapes(
            "
use system.collections.list

enum Tok
    Num(int)
    Real(float)

struct In
    a int
    b float

struct Mid
    inner In
    k i8

enum Deep
    Hold(Mid)
    Nothing

fn rotate(t (Dir, (Shape, Tok))) (Dir, (Shape, Tok))
    let inner = t.1
    return (turn(t.0), (Shape.Circle(area(inner.0)), inner.1))

fn bump(d Deep) Deep
    match d
        Deep.Hold(m): Deep.Hold(Mid(inner: In(a: m.inner.a + 1, b: m.inner.b * 2.0), k: m.k))
        Deep.Nothing: Deep.Nothing

fn show(d Deep) String
    match d
        Deep.Hold(m): f\"{m.inner.a} {m.inner.b} {m.k}\"
        Deep.Nothing: \"nothing\"

fn main()
    var maybe = List<Shape?>()
    maybe.push(Shape.Circle(1.0))
    maybe.push(None)
    maybe.push(Shape.Rect(2.0, 3.0))
    maybe[0] = None
    maybe[1] = Shape.Circle(2.0)
    for m in maybe
        match m
            Some(s): println(f\"some {area(s)}\")
            None: println(\"none\")
    var arr = [Shape.Circle(1.0), Shape.Rect(1.0, 2.0), Shape.Circle(0.5)]
    arr[1] = Shape.Circle(3.0)
    var sum = 0.0
    for s in arr
        sum += area(s)
    println(f\"{sum}\")
    let t = rotate((Dir.West, (Shape.Rect(2.0, 2.0), Tok.Real(0.5))))
    let back = t.1
    println(f\"{t.0 == Dir.North} {area(back.0)} {back.1 == Tok.Real(0.5)}\")
    var mid = Mid(inner: In(a: 1, b: 1.5), k: 3)
    var d = Deep.Hold(mid)
    mid.inner.a = 40
    let copy = d
    d = bump(d)
    println(f\"{show(copy)} | {show(d)} | {show(Deep.Hold(mid))}\")
    println(f\"{copy == Deep.Hold(Mid(inner: In(a: 1, b: 1.5), k: 3))} {d == copy} {d == Deep.Hold(Mid(inner: In(a: 2, b: 3.0), k: 3))}\")
",
        ),
        "none\nsome 12.0\nsome 6.0\n30.75\ntrue 48.0 true\n1 1.5 3 | 2 3.0 3 | 40 1.5 3\ntrue false true",
    );
}

/// An enum held inside another's column is built and matched a thousand
/// times without an allocation.
#[test]
fn test_a_held_enum_nested_in_another_allocates_nothing() {
    assert_runs_allocating(
        "
use system.io

enum Inner
    A(int)
    B(float, int)

enum Outer
    Wrap(Inner)
    Pair(Inner, Inner)
    Empty

fn weigh(o Outer) int
    match o
        Outer.Wrap(Inner.A(n)): n
        Outer.Wrap(Inner.B(_, n)): n
        Outer.Pair(Inner.A(x), _): x
        Outer.Pair(_, _): 0
        Outer.Empty: 0

fn main()
    var total = 0
    var i = 0
    while i < 1000
        let o = if i % 2 == 0: Outer.Wrap(Inner.A(i)) else: Outer.Pair(Inner.A(1), Inner.B(1.0, i))
        total += weigh(o)
        i += 1
    if total == 250000
        println(\"weighed\")
",
        "weighed",
        0,
    );
}

/// A held enum in a struct field rewritten a thousand times allocates
/// nothing.
#[test]
fn test_a_held_enum_field_rewritten_in_a_loop_allocates_nothing() {
    assert_runs_allocating(
        &with_shapes(
            "
struct Piece
    id int
    facing Dir
    shape Shape

fn main()
    var p = Piece(id: 1, facing: Dir.North, shape: Shape.Circle(1.0))
    var i = 0
    while i < 1000
        p.facing = turn(p.facing)
        p.shape = if i % 2 == 0: Shape.Rect(1.0, i as float) else: Shape.Circle(1.0)
        i += 1
    if p.facing == Dir.North and area(p.shape) == 3.0
        println(\"turned\")
",
        ),
        "turned",
        0,
    );
}

/// What a set of held enums allocates is what a set of `int` keeping as
/// many elements allocates: no element is an allocation of its own.
#[test]
fn test_a_set_of_held_enums_allocates_what_a_set_of_ints_does() {
    let dirs = with_shapes(
        "
use system.collections.set

fn main()
    var seen = Set<Dir>()
    var d = Dir.North
    var i = 0
    while i < 1000
        seen.add(d)
        d = turn(d)
        i += 1
    if seen.length() == 4
        println(\"seen\")
",
    );
    let ints = "
use system.io
use system.collections.set

fn main()
    var seen = Set<int>()
    var i = 0
    while i < 1000
        seen.add(i % 4)
        i += 1
    if seen.length() == 4
        println(\"seen\")
";
    assert_runs_allocating(&dirs, "seen", SET_OF_FOUR_ALLOCATIONS);
    assert_runs_allocating(ints, "seen", SET_OF_FOUR_ALLOCATIONS);
}

/// What one set keeping four elements allocates: the set and the two
/// buffers it grows into.
const SET_OF_FOUR_ALLOCATIONS: usize = 3;

/// Two lists of `int` grown to 1000 elements allocate what the two lists of
/// held enums do, which pins [`LIST_BUFFER_ALLOCATIONS`] to the buffers.
#[test]
fn test_lists_of_ints_allocate_the_same_buffers() {
    assert_runs_allocating(
        "
use system.io
use system.collections.list

fn main()
    var xs = List<int>()
    var ys = List<int>()
    var i = 0
    while i < 1000
        xs.push(i)
        ys.push(i)
        i += 1
    if xs.length() == 1000 and ys[999] == 999
        println(\"listed\")
",
        "listed",
        LIST_BUFFER_ALLOCATIONS,
    );
}

/// A generic enum stays a counted allocation: building one in a loop
/// allocates each value, one allocation per iteration.
#[test]
fn test_a_generic_enum_built_in_a_loop_allocates_each_value() {
    assert_runs_allocating(
        "
use system.io

enum Either<T>
    One(T)
    Two(T, T)

fn first<T>(e Either<T>) T
    match e
        Either.One(v): v
        Either.Two(v, _): v

fn main()
    var total = 0
    var i = 0
    while i < 1000
        let e Either<int> = if i % 2 == 0: Either.One(i) else: Either.Two(i, 0)
        total += first(e)
        i += 1
    if total == 499500
        println(\"counted\")
",
        "counted",
        1000,
    );
}

/// The widest enum held as its bytes, a tag beside fifteen `int` columns,
/// is built, copied, passed, compared and kept in a list.
#[test]
fn test_the_widest_held_enum_keeps_every_column() {
    let ints = (1..=15)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let names = (1..=15)
        .map(|i| format!("v{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sum = (1..=15)
        .map(|i| format!("v{i}"))
        .collect::<Vec<_>>()
        .join(" + ");
    let types = vec!["int"; 15].join(", ");
    let code = format!(
        "
use system.io
use system.collections.list

enum Wide
    Full({types})
    Bare

fn total(w Wide) int
    match w
        Wide.Full({names}): {sum}
        Wide.Bare: 0

fn main()
    let w = Wide.Full({ints})
    let copy = w
    var xs = List<Wide>()
    xs.push(copy)
    xs.push(Wide.Bare)
    println(f\"{{total(w)}} {{total(xs[0])}} {{w == xs[0]}} {{xs[1] == Wide.Bare}}\")
"
    );
    assert_heap_guard_output(&code, "120 120 true true");
}

/// Columns overlaying a `bool`, an `i128` and a `u8`, then an `f32`, an
/// `int` and an `i16`, read back each width the variant stored, under guards
/// and literal sub-patterns; an `i128` differing only in its high word is
/// unequal, and a narrower variant overwriting a wider one equals it built
/// fresh.
#[test]
fn test_columns_of_mixed_widths_read_each_width_back() {
    assert_heap_guard_output(
        "
use system.io
use system.collections.set

enum Col
    Flag(bool, f32)
    Big(i128, int)
    Byte(u8, i16, int)

enum Tok
    Num(int)
    Real(float)

fn describe(c Col) String
    match c
        Col.Flag(b, f) if b: f\"on {f}\"
        Col.Flag(_, f): f\"off {f}\"
        Col.Byte(u, s, 0): f\"zero tail {u} {s}\"
        Col.Byte(u, s, n) if s < 0: f\"neg byte {u} {s} {n}\"
        Col.Byte(u, s, n): f\"byte {u} {s} {n}\"
        Col.Big(_, n): f\"big {n}\"

fn is_zero(t Tok) bool
    match t
        Tok.Num(0): true
        Tok.Num(_): false
        Tok.Real(_): false

fn main()
    let w i128 = 4294967296
    let two64 = w * w
    let a = w * 1000
    let b = a + two64
    let items = [Col.Flag(true, 2.5 as f32), Col.Flag(false, 0.5 as f32), Col.Byte(255, -2, 7), Col.Byte(1, 3, 0), Col.Byte(4, 3, 9), Col.Big(a, 1)]
    for c in items
        println(describe(c))
    println(f\"{Col.Big(a, 1) == Col.Big(a, 1)} {Col.Big(a, 1) == Col.Big(b, 1)} {is_zero(Tok.Num(0))} {is_zero(Tok.Num(3))} {is_zero(Tok.Real(0.0))}\")
    let minus i128 = -1
    var c = Col.Big(minus, -1)
    c = Col.Byte(255, -2, 7)
    var seen = Set<Col>()
    seen.add(c)
    seen.add(Col.Byte(255, -2, 7))
    println(f\"{c == Col.Byte(255, -2, 7)} {seen.length()} {describe(c)}\")
",
        "on 2.5\noff 0.5\nneg byte 255 -2 7\nzero tail 1 3\nbyte 4 3 9\nbig 1\ntrue false true false false\ntrue 1 neg byte 255 -2 7",
    );
}

/// An enum of one variant is held like any other: compared, kept in a set
/// and in a list, matched by its one arm.
#[test]
fn test_one_variant_enums_are_compared_kept_and_matched() {
    assert_heap_guard_output(
        "
use system.io
use system.collections.set
use system.collections.list

enum One
    Only

enum OneWide
    Only(float, int)

fn main()
    let a = One.Only
    let w = OneWide.Only(1.5, 2)
    var ones = Set<One>()
    ones.add(a)
    ones.add(One.Only)
    var wides = Set<OneWide>()
    wides.add(w)
    wides.add(OneWide.Only(1.5, 2))
    wides.add(OneWide.Only(1.5, 3))
    var xs = List<OneWide>()
    xs.push(w)
    xs.push(OneWide.Only(0.5, 1))
    xs[0] = OneWide.Only(2.5, 4)
    var total = 0.0
    for x in xs
        match x
            OneWide.Only(f, n): total += f * (n as float)
    println(f\"{a == One.Only} {w == OneWide.Only(1.5, 2)} {w == OneWide.Only(1.5, 3)} {ones.length()} {wides.length()} {total}\")
",
        "true true false 1 2 10.5",
    );
}

/// An enum carrying one that declares a drop hook is counted, so the hook
/// still runs once, after the copy, when the last holder lets go.
#[test]
fn test_an_enum_carrying_a_drop_hook_enum_runs_the_hook_once() {
    assert_heap_guard_output(
        "
use system.io

enum Handle
    Open(int)

    fn drop()
        match self
            Handle.Open(n): println(f\"closed {n}\")

enum Holder
    Has(Handle)
    Nothing

fn main()
    let a = Holder.Has(Handle.Open(1))
    let b = a
    match b
        Holder.Has(Handle.Open(n)): println(f\"held {n}\")
        Holder.Nothing: println(\"nothing\")
",
        "held 1\nclosed 1",
    );
}

/// An enum reaching itself stays counted and is built and walked.
#[test]
fn test_a_recursive_enum_is_built_and_walked() {
    assert_heap_guard_output(
        "
use system.io

enum Tree
    Leaf(int)
    Node(Tree, Tree)

fn sum(t Tree) int
    match t
        Tree.Leaf(n): n
        Tree.Node(l, r): sum(l) + sum(r)

fn main()
    let t = Tree.Node(Tree.Leaf(1), Tree.Node(Tree.Leaf(2), Tree.Leaf(3)))
    println(f\"{sum(t)}\")
",
        "6",
    );
}

#[test]
fn test_a_match_missing_a_variant_of_a_held_enum_is_refused() {
    assert_compiler_error(
        &with_shapes(
            "
fn widest(s Shape) float
    match s
        Shape.Circle(r): r

fn main()
    println(f\"{widest(Shape.Circle(1.0))}\")
",
        ),
        "Non-exhaustive match on Enum 'Shape'. Missing variants: Rect",
    );
}
