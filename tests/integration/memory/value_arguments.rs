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

/// The value types the in-place tests share: a vector, and a sphere holding
/// one inline.
const SPHERE: &str = r#"use system.math

struct Vec3
    x float
    y float
    z float

struct Sphere
    center Vec3
    r float

fn len(v Vec3) float
    sqrt(v.x * v.x + v.y * v.y + v.z * v.z)
"#;

/// `program` after the shared [`SPHERE`] declarations.
fn with_sphere(program: &str) -> String {
    format!("{SPHERE}{program}")
}

/// The argument at `position` of every call `function` makes to `callee` in
/// `program`, in order, as `miri build --emit mir` writes them.
fn arguments_at(program: &str, function: &str, callee: &str, position: usize) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("prog.mi");
    std::fs::write(&source, program).unwrap();
    let out = dir.path().join("prog");
    crate::utils::miri_cmd()
        .arg("build")
        .arg(&source)
        .arg("--out")
        .arg(&out)
        .args(["--emit", "mir"])
        .assert()
        .success();
    let mir = std::fs::read_to_string(dir.path().join("prog.mir")).unwrap();
    let header = format!("fn {function}\n");
    let start = mir
        .find(&header)
        .unwrap_or_else(|| panic!("no `{function}` in:\n{mir}"));
    let body = &mir[start + header.len()..];
    let body = &body[..body.find("\nfn ").unwrap_or(body.len())];
    let call = format!("fn {callee}(");
    let arguments: Vec<String> = body
        .match_indices(&call)
        .map(|(at, _)| {
            let rest = &body[at + call.len()..];
            let list = &rest[..rest.find(')').unwrap_or(rest.len())];
            list.split(", ")
                .nth(position)
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert!(!arguments.is_empty(), "no call of `{callee}` in:\n{body}");
    arguments
}

/// Whether `operand` is spelled as a local reached through `fields`, such
/// as `_2.0.1` for `[0, 1]`.
fn is_field_path(operand: &str, fields: &[usize]) -> bool {
    let mut steps = operand.split('.');
    let root = steps.next().unwrap_or_default();
    let path: Vec<String> = steps.map(str::to_string).collect();
    is_whole_local(root) && path == fields.iter().map(usize::to_string).collect::<Vec<_>>()
}

/// Whether `operand` is spelled as a whole local, such as `_17`.
fn is_whole_local(operand: &str) -> bool {
    operand
        .strip_prefix('_')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Every call `function` makes to `callee` in `program` takes, at
/// `position`, the place reached through `fields`, read in place.
fn assert_passed_in_place(
    program: &str,
    (function, callee, position): (&str, &str, usize),
    fields: &[usize],
) {
    let arguments = arguments_at(program, function, callee, position);
    assert!(
        arguments
            .iter()
            .all(|argument| is_field_path(argument, fields)),
        "expected `{callee}` to read a field path {fields:?} in place, got {arguments:?}"
    );
}

/// The first call `function` makes to `callee` in `program` takes, at
/// `position`, a whole local: the copy made for it.
fn assert_copied(program: &str, (function, callee, position): (&str, &str, usize)) {
    let arguments = arguments_at(program, function, callee, position);
    assert!(
        is_whole_local(&arguments[0]),
        "expected `{callee}` to read a copy, got {arguments:?}"
    );
}

/// `len(s.center)` reads the vector straight out of the sphere's own bytes:
/// no copy local is made for it.
#[test]
fn a_field_of_a_value_local_is_passed_from_the_locals_own_bytes() {
    let program = with_sphere(
        r#"
fn main()
    let s = Sphere(center: Vec3(x: 3.0, y: 0.0, z: 4.0), r: 1.0)
    println(f"{len(s.center)} {s.r}")
"#,
    );
    assert_runs_with_output(&program, "5.0 1.0");
    assert_passed_in_place(&program, ("main", "miri.len", 0), &[0]);
}

/// `len(w.v)` with `w.v` after a `bool`: the callee reads the vector at its
/// offset inside `w`, not at `w`'s first byte.
#[test]
fn a_field_at_a_nonzero_offset_is_passed_from_its_own_offset() {
    let program = with_sphere(
        r#"
struct W
    flag bool
    v Vec3
    n int

fn main()
    let w = W(flag: true, v: Vec3(x: 0.0, y: 3.0, z: 4.0), n: 7)
    println(f"{len(w.v)} {w.flag} {w.n}")
"#,
    );
    assert_runs_with_output(&program, "5.0 true 7");
    assert_passed_in_place(&program, ("main", "miri.len", 0), &[1]);
}

/// `both(s, s.center)`: the callee copies `s` and writes the copy's centre
/// while it holds the caller's centre in place, which stays as it was.
#[test]
fn a_field_passed_in_place_survives_the_callee_writing_a_copy_of_its_holder() {
    let program = with_sphere(
        r#"
fn both(s Sphere, c Vec3) float
    var t = s
    t.center = Vec3(x: 9.0, y: 9.0, z: 9.0)
    c.x + t.center.x

fn main()
    let s = Sphere(center: Vec3(x: 1.0, y: 2.0, z: 3.0), r: 1.0)
    println(f"{both(s, s.center)} {s.center.x}")
"#,
    );
    assert_runs_with_output(&program, "10.0 1.0");
    assert_passed_in_place(&program, ("main", "miri.both", 1), &[0]);
}

/// `s.center = swap(s.center)` returns into a temporary that is stored into
/// `s.center` after the call, so the call itself writes nothing of `s` and
/// reads the centre in place.
#[test]
fn a_field_assigned_a_call_on_itself_is_read_in_place() {
    let program = with_sphere(
        r#"
fn swap(v Vec3) Vec3
    Vec3(x: v.z, y: v.x, z: v.y)

fn main()
    var s = Sphere(center: Vec3(x: 1.0, y: 2.0, z: 3.0), r: 1.0)
    s.center = swap(s.center)
    println(f"{s.center.x} {s.center.y} {s.center.z}")
"#,
    );
    assert_runs_with_output(&program, "3.0 1.0 2.0");
    assert_passed_in_place(&program, ("main", "miri.swap", 0), &[0]);
}

/// `set(s.center, s)` with `s` passed `out`: the callee writes the centre
/// before it reads its value argument, which still holds the old bytes.
#[test]
fn a_field_of_an_out_argument_is_still_copied() {
    let program = with_sphere(
        r#"
fn set(a Vec3, o out Sphere)
    o.center.x = 100.0
    o.r = a.x + a.y

fn main()
    var s = Sphere(center: Vec3(x: 3.0, y: 4.0, z: 0.0), r: 1.0)
    set(s.center, s)
    println(f"{s.center.x} {s.r}")
"#,
    );
    assert_runs_with_output(&program, "100.0 7.0");
    assert_copied(&program, ("main", "miri.set", 0));
}

/// `len(l.a.center)` two fields deep, and `len(s.center)` off a value
/// parameter, each read in place.
#[test]
fn nested_and_parameter_rooted_fields_are_passed_in_place() {
    let program = with_sphere(
        r#"
struct Line
    a Sphere
    b Sphere

fn radius_of(s Sphere) float
    len(s.center) + s.r

fn main()
    let l = Line(a: Sphere(center: Vec3(x: 0.0, y: 3.0, z: 4.0), r: 2.0), b: Sphere(center: Vec3(x: 6.0, y: 8.0, z: 0.0), r: 1.0))
    println(f"{len(l.a.center)} {radius_of(l.b)}")
"#,
    );
    assert_runs_with_output(&program, "5.0 11.0");
    assert_passed_in_place(&program, ("main", "miri.len", 0), &[0, 0]);
    assert_passed_in_place(&program, ("main", "miri.radius_of", 0), &[1]);
    assert_passed_in_place(&program, ("miri.radius_of", "miri.len", 0), &[0]);
}

/// `norm_or(h.o, ..)`: an optional value held inline in a struct is passed
/// from the struct's own bytes, present and absent.
#[test]
fn an_optional_field_of_a_value_local_is_passed_in_place() {
    let program = with_sphere(
        r#"
struct Holder
    o Vec3?
    k int

fn norm_or(v Vec3?, fallback float) float
    match v
        Some(p): len(p)
        None: fallback

fn main()
    let h = Holder(o: Vec3(x: 0.0, y: 6.0, z: 8.0), k: 1)
    let e = Holder(o: None, k: 2)
    println(f"{norm_or(h.o, -1.0)} {norm_or(e.o, -1.0)} {h.k + e.k}")
"#,
    );
    assert_runs_with_output(&program, "10.0 -1.0 3");
    assert_passed_in_place(&program, ("main", "miri.norm_or", 0), &[0]);
}

/// `len(t[i])`: a member picked by a runtime index sits at no fixed offset
/// of the tuple, so it is still copied.
#[test]
fn a_tuple_member_picked_by_a_runtime_index_is_still_copied() {
    let program = with_sphere(
        r#"
fn main()
    let t = (Vec3(x: 3.0, y: 0.0, z: 4.0), Vec3(x: 0.0, y: 6.0, z: 8.0))
    var total = 0.0
    for i in 0..2
        total = total + len(t[i])
    println(f"{total}")
"#,
    );
    assert_runs_with_output(&program, "15.0");
    assert_copied(&program, ("main", "miri.len", 0));
}

/// `len(t.0)`: a tuple member named by its position sits at a fixed offset,
/// so it is read in place.
#[test]
fn a_tuple_member_named_by_its_position_is_passed_in_place() {
    let program = with_sphere(
        r#"
fn main()
    let t = (2.0, Vec3(x: 0.0, y: 6.0, z: 8.0))
    println(f"{len(t.1)} {t.0}")
"#,
    );
    assert_runs_with_output(&program, "10.0 2.0");
    assert_passed_in_place(&program, ("main", "miri.len", 0), &[1]);
}

/// `id(s.center)` hands its in-place parameter back through its return
/// slot, which is a local of its own.
#[test]
fn a_field_passed_in_place_and_returned_is_a_copy() {
    let program = with_sphere(
        r#"
fn id(v Vec3) Vec3
    v

fn main()
    var s = Sphere(center: Vec3(x: 3.0, y: 0.0, z: 4.0), r: 1.0)
    let c = id(s.center)
    s.center.z = 0.0
    println(f"{c.z} {s.center.z}")
"#,
    );
    assert_runs_with_output(&program, "4.0 0.0");
    assert_passed_in_place(&program, ("main", "miri.id", 0), &[0]);
}

/// A method and a call through a trait each take a field of a value local
/// beside their receiver, in place.
#[test]
fn a_field_passed_to_a_method_beside_its_receiver_is_read_in_place() {
    let program = with_sphere(
        r#"
class Meter
    var k float
    fn measure(self, v Vec3) float
        self.k * len(v)

trait Gauge
    fn read(self, v Vec3) float

class Ruler implements Gauge
    var k float
    fn read(self, v Vec3) float
        self.k + v.z

fn gauge(g Gauge, s Sphere) float
    g.read(s.center)

fn main()
    let s = Sphere(center: Vec3(x: 3.0, y: 0.0, z: 4.0), r: 1.0)
    println(f"{Meter(k: 2.0).measure(s.center)} {gauge(Ruler(k: 1.0), s)}")
"#,
    );
    assert_runs_with_output(&program, "10.0 5.0");
    assert_passed_in_place(&program, ("main", "miri.Meter.measure", 1), &[0]);
}

/// `s = xs[0]` then `len(s.center)`: the list writes the element into `s`
/// through its address, taken only for that runtime call, and the field is
/// still read in place by the Miri calls before and after it.
#[test]
fn a_field_of_a_local_a_list_read_fills_is_passed_in_place() {
    let program = with_sphere(
        r#"
use system.collections.list

fn main()
    let xs = List<Sphere>()
    var s = Sphere(center: Vec3(x: 3.0, y: 0.0, z: 4.0), r: 1.0)
    xs.push(s)
    let before = len(s.center)
    s = xs[0]
    s.center.x = 0.0
    println(f"{before} {len(s.center)} {xs[0].center.x} {xs.length()}")
"#,
    );
    assert_runs_with_output(&program, "5.0 4.0 3.0 1");
    let arguments = arguments_at(&program, "main", "miri.len", 0);
    assert_eq!(arguments.len(), 2, "{arguments:?}");
    assert!(
        arguments
            .iter()
            .all(|argument| is_field_path(argument, &[0])),
        "{arguments:?}"
    );
}
