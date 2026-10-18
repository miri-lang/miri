// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional around a scalar or a value held as its bytes is itself held as
//! bytes — a presence tag and the payload beside it — wherever it lives: a
//! local, an argument, a result, a field, an element. Building one, matching
//! one and handing one back allocates nothing.
//!
//! Each test checks the program's output and that no allocation of the kind
//! an optional's box was counted as happened (`inline` in the allocation
//! breakdown).

use super::super::utils::*;

/// Runs `code`, checks its output, and checks that it allocated no box.
fn assert_runs_without_boxes(code: &str, expected: &str) {
    assert_runs_with_boxes(code, expected, 0);
}

/// Runs `code`, checks its output, and checks that it allocated exactly
/// `boxes` allocations of the kind an optional's box is counted as — the
/// objects and closure environments the program builds itself, and no
/// optional.
fn assert_runs_with_boxes(code: &str, expected: &str, boxes: usize) {
    assert_runs_with_output(code, expected);
    let breakdown = parse_alloc_count_breakdown(code);
    assert_eq!(
        breakdown.inline, boxes,
        "the program allocated {} boxes (rc={} buffers={} raw={})",
        breakdown.inline, breakdown.rc, breakdown.buffers, breakdown.raw
    );
}

#[test]
fn test_popping_an_int_list_allocates_no_optional() {
    assert_runs_without_boxes(
        r#"
use system.io
use system.collections.list

fn main()
    var xs = List<int>()
    for i in 0..1000
        xs.push(i)
    var s = 0
    for i in 0..1000
        s = s + (xs.pop() ?? 0)
    let missing = xs.pop() ?? -1
    println(f"{s} {missing}")
"#,
        "499500 -1",
    );
}

#[test]
fn test_reading_the_codes_of_a_string_allocates_no_optional() {
    assert_runs_without_boxes(
        r#"
use system.io

fn main()
    var text = ""
    for i in 0..1000
        text = text + "a"
    var s = 0
    for i in 0..1000
        s = s + (text.code_at(i) ?? 0)
    let missing = text.code_at(1000) ?? -1
    println(f"{s} {missing}")
"#,
        "97000 -1",
    );
}

#[test]
fn test_looking_up_an_int_in_a_map_allocates_no_optional() {
    assert_runs_without_boxes(
        r#"
use system.io
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["a"] = 1
    var s = 0
    for i in 0..1000
        s = s + (m.get("a") ?? 0)
    let missing = m.get("b") ?? -1
    println(f"{s} {missing}")
"#,
        "1000 -1",
    );
}

#[test]
fn test_a_doubly_optional_int_keeps_some_none_apart_from_none() {
    assert_runs_without_boxes(
        r#"
use system.io

fn wrap(x int?) Option<int?>: Some(x)

fn describe(v Option<int?>) String
    match v
        Some(inner)
            match inner
                Some(n): return f"some {n}"
                None: return "some none"
        None: return "none"

fn main()
    let empty Option<int?> = None
    let wrapped_none = wrap(None)
    println(describe(wrapped_none))
    println(describe(wrap(Some(3))))
    println(describe(empty))
    println(f"{wrapped_none == None} {empty == None}")
"#,
        "some none\nsome 3\nnone\nfalse true",
    );
}

#[test]
fn test_a_set_of_optional_ints_finds_present_and_absent_members() {
    assert_runs_without_boxes(
        r#"
use system.io
use system.collections.set

fn main()
    var s = Set<int?>()
    s.add(Some(1))
    s.add(None)
    s.add(Some(1))
    let one int? = Some(1)
    let two int? = Some(2)
    let nothing int? = None
    println(f"{s.length()} {s.contains(one)} {s.contains(nothing)} {s.contains(two)}")
"#,
        "2 true true false",
    );
}

#[test]
fn test_popping_a_list_of_generic_pairs_allocates_only_its_buffers() {
    let code = r#"
use system.io
use system.collections.list

struct Pair<A, B>
    a A
    b B

fn main()
    var xs = List<Pair<int, float>>()
    var s = 0
    for i in 0..1000
        xs.push(Pair<int, float>(a: i, b: 1.5))
        match xs.pop()
            Some(p): s = s + p.a
            None: s = s - 1
    println(f"{s}")
"#;
    assert_runs_without_boxes(code, "499500");
    assert!(get_allocation_count_value(code) <= 8);
}

#[test]
fn test_a_struct_holding_an_optional_int_is_held_as_its_bytes() {
    assert_runs_without_boxes(
        r#"
use system.io
use system.collections.list

struct S
    a int
    b int?

fn main()
    var xs = List<S>()
    for i in 0..100
        xs.push(S(a: i, b: Some(i)))
    xs.push(S(a: 1, b: None))
    var s = 0
    for x in xs
        s = s + x.a + (x.b ?? 0)
    println(f"{s}")
"#,
        "9901",
    );
}

/// The point type and the optionals of it the call tests hand around.
const POINT: &str = r#"
use system.io

struct P
    x int
    y int

fn show(n int?, p P?) String
    let a = n ?? -1
    match p
        Some(q): return f"{a} {q.x},{q.y}"
        None: return f"{a} none"
"#;

fn with_point(body: &str) -> String {
    format!("{POINT}\n{body}")
}

#[test]
fn test_optionals_cross_a_direct_call() {
    assert_runs_without_boxes(
        &with_point(
            r#"
fn swap(n int?, p P?) P?
    match p
        Some(q): return P(x: q.y + (n ?? 0), y: q.x)
        None: return None

fn bump(n int?) int?
    match n
        Some(v): return v + 1
        None: return None

fn main()
    println(show(bump(4), swap(Some(1), P(x: 2, y: 3))))
    println(show(bump(None), swap(None, None)))
"#,
        ),
        "5 4,2\n-1 none",
    );
}

#[test]
fn test_optionals_cross_a_virtual_call() {
    // The one box is the `Right` object.
    assert_runs_with_boxes(
        &with_point(
            r#"
trait Shift
    fn bump(n int?) int?
    fn moved(p P?) P?

class Right implements Shift
    dx int
    fn bump(n int?) int?
        match n
            Some(v): return v + self.dx
            None: return None
    fn moved(p P?) P?
        match p
            Some(q): return P(x: q.x + self.dx, y: q.y)
            None: return None

fn run(s Shift)
    println(show(s.bump(1), s.moved(P(x: 1, y: 2))))
    println(show(s.bump(None), s.moved(None)))

fn main()
    run(Right(dx: 10))
"#,
        ),
        "11 11,2\n-1 none",
        1,
    );
}

#[test]
fn test_optionals_cross_a_closure_call() {
    // The two boxes are the closures' environments.
    assert_runs_with_boxes(
        &with_point(
            r#"
fn shifted(p P?, k int) P?
    match p
        Some(q): return P(x: q.y + k, y: q.x)
        None: return None

fn main()
    let k = 5
    let bump = fn(n int?) int?: Some((n ?? 0) + k)
    let flip = fn(p P?) P?: shifted(p, k)
    println(show(bump(1), flip(P(x: 1, y: 2))))
    println(show(bump(None), flip(None)))
"#,
        ),
        "6 7,1\n5 none",
        2,
    );
}

#[test]
fn test_optionals_cross_a_call_through_a_function_value() {
    // The four boxes are the function values handed to `apply`, two per call.
    assert_runs_with_boxes(
        &with_point(
            r#"
fn bump(n int?) int?: Some((n ?? 0) + 1)

fn flip(p P?) P?
    match p
        Some(q): return P(x: q.y, y: q.x)
        None: return None

fn apply(f fn(n int?) int?, g fn(p P?) P?, n int?, p P?) String
    return show(f(n), g(p))

fn main()
    println(apply(bump, flip, 1, P(x: 1, y: 2)))
    println(apply(bump, flip, None, None))
"#,
        ),
        "2 2,1\n1 none",
        4,
    );
}

/// `Some(None)` is spelled at a placeholder type before it meets a slot. Read
/// into an `int??` held as its bytes — a local, an argument, a field, an
/// element, a set member — it keeps its layers, and the placeholder is
/// released once.
#[test]
fn test_a_nested_absent_value_meets_every_slot_under_the_heap_guard() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.set

struct Cell
    v Option<int?>

fn depth(v Option<int?>) int
    match v
        Some(inner)
            match inner
                Some(_): return 2
                None: return 1
        None: return 0

fn main()
    let here Option<int?> = Some(None)
    let cell = Cell(v: Some(None))
    var xs = List<Option<int?>>()
    xs.push(Some(None))
    var s = Set<Option<int?>>()
    s.add(Some(None))
    s.add(Some(Some(4)))
    println(f"{depth(here)} {depth(Some(None))} {depth(cell.v)} {depth(xs[0])}")
    println(f"{s.length()} {s.contains(Some(None))} {s.contains(None)}")
"#,
        "1 1 1 1\n2 true false",
    );
}

#[test]
fn test_optionals_captured_by_a_closure_are_copied_with_it() {
    // The one box is the closure's environment, which holds the optionals'
    // bytes.
    assert_runs_with_boxes(
        &with_point(
            r#"
fn main()
    let k int? = Some(5)
    let n int? = None
    let p P? = P(x: 1, y: 2)
    let f = fn() String: show((k ?? 0) + (n ?? 10), p)
    println(f())
"#,
        ),
        "15 1,2",
        1,
    );
}

#[test]
fn test_optionals_of_every_scalar_width_keep_their_values() {
    assert_runs_without_boxes(
        r#"
use system.io

fn id8(x u8?) u8?: x

fn main()
    let a i8 = -3
    let oa i8? = a
    let b u8 = 200
    let ob u8? = id8(b)
    let oc bool? = true
    let d f32 = 1.5
    let od f32? = d
    let e i128 = 7
    let oe i128? = e
    let missing u8? = id8(None)
    println(f"{oa ?? 0} {ob ?? 0} {oc ?? false} {od ?? 0.0} {oe ?? 0} {missing ?? 9}")
"#,
        "-3 200 true 1.5 7 9",
    );
}

/// A struct of fourteen `int`s and an `int?` is 128 bytes, the widest value
/// held as its bytes; one more `int` makes it a counted allocation. Both keep
/// their optional field.
#[test]
fn test_a_struct_with_an_optional_field_at_the_value_size_limit() {
    let fields = |count: usize| {
        (0..count)
            .map(|index| format!("    f{index} int\n"))
            .collect::<String>()
    };
    let build = |count: usize| {
        (0..count)
            .map(|index| format!("f{index}: {index}, "))
            .collect::<String>()
    };
    let code = format!(
        r#"
use system.io
use system.collections.list

struct Wide
{wide_fields}    o int?

struct Wider
{wider_fields}    o int?

fn main()
    var ws = List<Wide>()
    ws.push(Wide({wide_build}o: Some(7)))
    ws.push(Wide({wide_build}o: None))
    var vs = List<Wider>()
    vs.push(Wider({wider_build}o: 8))
    let w = ws[0]
    let v = vs[0]
    println(f"{{w.f13}} {{w.o ?? -1}} {{ws[1].o ?? -1}} {{v.f14}} {{v.o ?? -1}}")
"#,
        wide_fields = fields(14),
        wider_fields = fields(15),
        wide_build = build(14),
        wider_build = build(15),
    );
    assert_heap_guard_output(&code, "13 7 -1 14 8");
}

#[test]
fn test_an_optional_set_in_a_loop_survives_break_and_continue() {
    assert_runs_without_boxes(
        r#"
use system.io

fn main()
    var found int? = None
    var i = 0
    while i < 10
        i = i + 1
        if i % 2 == 0
            continue
        if i == 7
            found = i
            break
    var total = 0
    for x in [1, 2, 3]
        var seen int?
        if x != 2
            seen = x
        total = total + (seen ?? 100)
    println(f"{found ?? -1} {i} {total}")
"#,
        "7 7 104",
    );
}
