// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An argument to an `out` parameter may be a place path: a field or an
//! element reached from a `var`, an `out` parameter or a method's own
//! receiver. The place is taken for the call and restored on every exit: a
//! counted value or one held as its bytes is handed over by the address of
//! its slot, so the callee writes it in place, and a scalar is copied in and
//! back out. Every copy-on-write block above the slot is made the caller's
//! own once, before the call. What is refused, and the repair each refusal
//! names, is pinned beside this file in `out_places_refused.rs`.

use super::super::utils::*;
use super::lent_field_arguments::emitted_body;
use super::rc_stats::rc_stats;

const CELL: &str = r#"
use system.io
use system.memory
use system.collections.list

class Cell
    var n int

fn step(c out Cell)
    c = Cell(n: c.n + 10)
"#;

const CONN: &str = r#"
use system.io
use system.memory
use system.collections.list

class Conn
    var id int
    fn drop()
        println(f"closed {self.id}")
"#;

fn with_cell(program: &str) -> String {
    format!("{CELL}\n{program}")
}

fn with_conn(program: &str) -> String {
    format!("{CONN}\n{program}")
}

#[test]
fn an_element_of_a_list_of_counted_cells_is_written_in_place() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn main()
    var grid = List([Cell(n: 1), Cell(n: 2)])
    let i = 1
    step(grid[i])
    println(f"{grid[0].n} {grid[1].n}")
"#,
        ),
        "1 12",
    );
}

#[test]
fn an_element_handed_out_is_its_slot_and_no_copy_of_it() {
    let body = emitted_body(
        &with_cell(
            r#"
fn main()
    var grid = List([Cell(n: 1), Cell(n: 2)])
    let i = 1
    step(grid[i])
    println(f"{grid[1].n}")
"#,
        ),
        "main",
    );
    let call = (body.lines())
        .find(|line| line.contains("fn miri.step("))
        .unwrap_or_else(|| panic!("no call to step in:\n{body}"));
    let argument = call
        .split("fn miri.step(")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .unwrap_or_default();
    assert!(
        argument.starts_with("out _") && argument.contains('[') && argument.ends_with(']'),
        "the element should be handed over as its slot: {call}\n{body}"
    );
    let slot = argument.trim_start_matches("out ");
    assert!(
        !body.contains(&format!("= {slot};")) && !body.contains(&format!("= {slot}\n")),
        "the element should not be read into a temporary for the call:\n{body}"
    );
}

/// Stepping one of ten cells a thousand times or a hundred thousand times
/// retains exactly as often: handing an element over by its slot retains
/// nothing.
#[test]
fn stepping_an_element_in_a_loop_retains_nothing_per_iteration() {
    let program = |rounds: u32| {
        with_cell(&format!(
            r#"
fn bump(c out Cell)
    c.n = c.n + 1

fn main()
    var grid = List<Cell>()
    for k in 0..10: grid.push(Cell(n: k))
    for k in 0..{rounds}: bump(grid[k % 10])
    println(f"{{grid[3].n}}")
"#
        ))
    };
    let dups = |line: &str| {
        line.split_whitespace()
            .find(|field| field.starts_with("dup="))
            .map(str::to_string)
            .unwrap_or_default()
    };
    let few = rc_stats(&program(1_000), &[]);
    let many = rc_stats(&program(100_000), &[]);
    assert_eq!(dups(&few), dups(&many), "{few}\n{many}");
}

#[test]
fn a_scalar_element_is_copied_in_and_back_out() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn bump(n out int)
    n = n + 1

fn main()
    var v = List([1, 2, 3])
    var w = v
    bump(v[1])
    bump(v[1])
    println(f"{v[0]} {v[1]} {v[2]} {w[1]}")
"#,
        "1 4 3 2",
    );
}

#[test]
fn a_shared_list_is_copied_once_before_its_element_is_handed_out() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn main()
    var grid = List([Cell(n: 1), Cell(n: 2)])
    let kept = grid
    step(grid[0])
    println(f"{grid[0].n} {kept[0].n}")
"#,
        ),
        "11 1",
    );
}

#[test]
fn a_field_of_self_is_replaced_inside_a_method() {
    assert_heap_guard_output(
        &with_conn(
            r#"
class Holder
    var slot Conn? = None

    fn put(c Conn)
        self.slot = c

    fn take_slot() Conn?
        return replace(self.slot, None)

fn main()
    var h = Holder()
    h.put(Conn(id: 1))
    let taken = h.take_slot()
    match taken
        Some(c): println(f"took {c.id}")
        None: println("empty")
    match h.slot
        Some(c): println(f"still {c.id}")
        None: println("now empty")
"#,
        ),
        "took 1\nnow empty\nclosed 1",
    );
}

#[test]
fn a_plain_resource_field_is_replaced_through_a_var() {
    assert_heap_guard_output(
        &with_conn(
            r#"
class Holder
    var conn Conn

fn main()
    var h = Holder(conn: Conn(id: 1))
    let old = replace(h.conn, Conn(id: 2))
    println(f"old {old.id} now {h.conn.id}")
"#,
        ),
        "old 1 now 2\nclosed 1\nclosed 2",
    );
}

#[test]
fn a_sibling_operand_reads_the_element_as_it_was_before_the_call() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn put(c out Cell, seen Cell)
    c = Cell(n: seen.n + 100)
    println(f"seen {seen.n}")

fn main()
    var v = List([Cell(n: 1), Cell(n: 2)])
    put(v[0], v[1])
    put(v[1], v[1])
    println(f"{v[0].n} {v[1].n}")
"#,
        ),
        "seen 2\nseen 2\n102 102",
    );
}

#[test]
fn an_early_return_in_the_callee_restores_the_place() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn settle(n out int, c out Cell, stop bool)
    n = 7
    c = Cell(n: 70)
    if stop
        return
    n = 8
    c = Cell(n: 80)

fn main()
    var v = List([1, 2])
    var grid = List([Cell(n: 1)])
    settle(v[0], grid[0], true)
    println(f"{v[0]} {grid[0].n}")
    settle(v[0], grid[0], false)
    println(f"{v[0]} {grid[0].n}")
"#,
        ),
        "7 70\n8 80",
    );
}

#[test]
fn an_early_err_return_in_the_callee_restores_the_place() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn grow(n out int, c out Cell) Result<int, String>
    n = n + 1
    c = Cell(n: c.n + 1)
    if n > 2
        return Result.Err("too big")
    return Result.Ok(n)

fn main()
    var v = List([1, 2])
    var grid = List([Cell(n: 1)])
    match grow(v[1], grid[0])
        Result.Ok(n): println(f"ok {n}")
        Result.Err(e): println(e)
    println(f"{v[1]} {grid[0].n}")
"#,
        ),
        "too big\n3 2",
    );
}

#[test]
fn a_generic_callee_takes_an_element_at_every_representation() {
    assert_heap_guard_output(
        &with_conn(
            r#"
struct Point
    x int
    y int

fn swap_in<T>(slot out T, value T) T
    return replace(slot, value)

fn main()
    var conns List<Conn?> = List([Some(Conn(id: 1))])
    var ints = List([1, 2])
    var points = List([Point(x: 1, y: 2)])
    let c0 = swap_in(conns[0], None)
    let n0 = swap_in(ints[1], 5)
    let p0 = swap_in(points[0], Point(x: 3, y: 4))
    println(f"{ints[1]} {n0} {points[0].x} {p0.x}")
    match c0
        Some(c): println(f"took {c.id}")
        None: println("none")
    match conns[0]
        Some(c): println(f"kept {c.id}")
        None: println("emptied")
"#,
        ),
        "5 2 3 1\ntook 1\nemptied\nclosed 1",
    );
}

#[test]
fn a_callee_storing_its_element_elsewhere_leaves_the_slot_shared() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn keep(xs out List<int>, store out List<List<int>>)
    store.push(xs)

fn main()
    var rows = List([List([1])])
    var store = List<List<int>>()
    rows[0].push(2)
    keep(rows[0], store)
    rows[0].push(3)
    println(f"{rows[0].length()} {store[0].length()}")
"#,
        "3 2",
    );
}

#[test]
fn a_nested_path_from_self_reaches_an_element() {
    assert_heap_guard_output(
        &with_cell(
            r#"
struct Inner
    cells List<Cell>

class Board
    var inner Inner

    fn bump(i int)
        step(self.inner.cells[i])

fn main()
    var board = Board(inner: Inner(cells: List([Cell(n: 1), Cell(n: 2)])))
    let kept = board.inner
    board.bump(1)
    println(f"{board.inner.cells[1].n} {kept.cells[1].n}")
"#,
        ),
        "12 2",
    );
}

#[test]
fn a_path_from_an_out_parameter_reaches_an_element() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn step_first(grid out List<Cell>)
    step(grid[0])

fn main()
    var grid = List([Cell(n: 1)])
    let kept = grid
    step_first(grid)
    println(f"{grid[0].n} {kept[0].n}")
"#,
        ),
        "11 1",
    );
}

#[test]
fn a_method_takes_an_element() {
    assert_heap_guard_output(
        &with_cell(
            r#"
class Stepper
    var by int

    fn apply(c out Cell)
        c = Cell(n: c.n + self.by)

    fn add(n out int)
        n = n + self.by

fn main()
    let s = Stepper(by: 5)
    var grid = List([Cell(n: 1), Cell(n: 2)])
    var ints = List([1, 2])
    s.apply(grid[1])
    s.add(ints[0])
    println(f"{grid[1].n} {ints[0]}")
"#,
        ),
        "7 6",
    );
}

/// A named function taken as a value is called through a thunk, which
/// writes its `out` parameter back to the caller's place as the function
/// does: a whole variable and an element alike.
#[test]
fn a_function_value_takes_a_variable_and_an_element() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn bump(n out int)
    n = n + 1

fn main()
    let f = bump
    var k = 1
    f(k)
    var v = List([1, 2])
    f(v[1])
    println(f"{k} {v[1]}")
"#,
        "2 3",
    );
}

const TAGGED: &str = r#"
use system.io
use system.collections.list

class Tagged
    var name String
    var tags List<String>
    var counts List<int>

fn retag(c out List<String>, h Tagged)
    c = List(["new"])
    println(f"h sees {h.tags[0]} {h.name}")

fn rename(s out String, h Tagged)
    s = "renamed"
    println(f"h sees {h.tags[0]}")

fn bump(n out int, h Tagged)
    n = n + 10
    println(f"h sees {h.counts[0]}")
"#;

fn with_tagged(program: &str) -> String {
    format!("{TAGGED}\n{program}")
}

/// A class instance is shared on assignment, so a second binding reaches
/// the slot a call takes through it: the slot is copied in, holding a
/// reference of its own, and stored back once the call returns, and the
/// callee reads the value it held before the call through the alias.
#[test]
fn a_field_of_a_class_instance_another_binding_shares_is_copied_in_and_out() {
    assert_heap_guard_output(
        &with_tagged(
            r#"
fn main()
    var h = Tagged(name: "x", tags: List(["old"]), counts: List([1]))
    var k = h
    h.name = "y"
    println(k.name)
    retag(h.tags, k)
    println(f"{h.tags[0]} {k.tags[0]}")
"#,
        ),
        "y\nh sees old y\nnew new",
    );
}

#[test]
fn an_element_inside_a_shared_class_instance_is_copied_in_and_out() {
    assert_heap_guard_output(
        &with_tagged(
            r#"
fn main()
    var h = Tagged(name: "x", tags: List(["old", "two"]), counts: List([1, 2]))
    var k = h
    rename(h.tags[1], k)
    bump(h.counts[0], k)
    println(f"{k.tags[1]} {k.counts[0]} {h.tags[0]}")
"#,
        ),
        "h sees old\nh sees 1\nrenamed 11 old",
    );
}

#[test]
fn an_element_of_a_list_inside_a_class_instance_in_a_list_is_written() {
    assert_heap_guard_output(
        &with_tagged(
            r#"
fn main()
    var all = List([Tagged(name: "a", tags: List(["t"]), counts: List([5, 6]))])
    let i = 0
    var plain = Tagged(name: "p", tags: List(["q"]), counts: List([0]))
    bump(all[i].counts[1], plain)
    println(f"{all[0].counts[1]}")
"#,
        ),
        "h sees 0\n16",
    );
}

#[test]
fn a_field_of_an_element_is_written_in_place() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Point
    x int
    label String

fn bump(n out int)
    n = n + 5

fn relabel(s out String)
    s = f"{s}!"

fn main()
    var points = List([Point(x: 1, label: "a"), Point(x: 2, label: "b")])
    let kept = points
    let i = 1
    bump(points[i].x)
    relabel(points[i].label)
    println(f"{points[1].x} {points[1].label} {kept[1].x} {kept[1].label}")
"#,
        "7 b! 2 b",
    );
}

const SHARED_TAGS: &str = r#"
use system.io
use system.collections.list

struct Tags
    tags List<String>

class Holder
    var tags List<String>
"#;

fn with_shared_tags(program: &str) -> String {
    format!("{SHARED_TAGS}\n{program}")
}

/// A static method takes a field of a class instance another binding
/// shares as any call does: copied in, holding a reference of its own,
/// and stored back once the call returns, while the alias it lends beside
/// the field still reads the value the field held before the call.
#[test]
fn a_static_method_takes_a_field_of_a_shared_class_instance() {
    assert_heap_guard_output(
        &with_shared_tags(
            r#"
class Store
    var id int
    public static fn fill(t out List<String>, k Holder)
        t = List<String>(["static"])
        println(f"k sees {k.tags[0]}")

fn main()
    var c = Holder(tags: List<String>(["h" + "i"]))
    let ck = c
    Store.fill(c.tags, ck)
    println(c.tags[0])
"#,
        ),
        "k sees hi\nstatic",
    );
}

/// A static method's write to an element lands in the caller's own copy of
/// the list: a `let` sharing the list keeps the value it held.
#[test]
fn a_static_method_takes_an_element_a_let_alias_shares() {
    assert_heap_guard_output(
        &with_shared_tags(
            r#"
class Store
    var id int
    public static fn fill(t out List<String>)
        t = List<String>(["static"])

fn main()
    var v = List([Tags(tags: List<String>(["a" + "b"]))])
    let w = v
    let i = 0
    Store.fill(v[i].tags)
    println(f"{v[0].tags[0]} {w[0].tags[0]}")
"#,
        ),
        "static ab",
    );
}

/// A static method of an enum takes an element as a class's does.
#[test]
fn an_enum_static_method_takes_an_element_a_let_alias_shares() {
    assert_heap_guard_output(
        &with_shared_tags(
            r#"
enum Source
    Disk
    Net
    public static fn fill(t out List<String>)
        t = List<String>(["enum"])

fn main()
    var v = List([Tags(tags: List<String>(["a" + "b"]))])
    let w = v
    Source.fill(v[0].tags)
    println(f"{v[0].tags[0]} {w[0].tags[0]}")
"#,
        ),
        "enum ab",
    );
}

/// A generic static method takes an element at the type its call binds.
#[test]
fn a_generic_static_method_takes_an_element() {
    assert_heap_guard_output(
        &with_shared_tags(
            r#"
class Store
    var id int
    public static fn put<T>(t out T, x T)
        t = x

fn main()
    var v = List([Tags(tags: List<String>(["a" + "b"]))])
    let w = v
    Store.put(v[0].tags, List<String>(["gen"]))
    var n = List([1, 2])
    Store.put(n[1], 7)
    println(f"{v[0].tags[0]} {w[0].tags[0]} {n[1]}")
"#,
        ),
        "gen ab 7",
    );
}

/// A default is a sibling operand: when it reads an earlier `out`
/// parameter handed a field, it sees the field's value before the call.
#[test]
fn a_default_reads_a_field_taken_out_as_it_was_before_the_call() {
    assert_heap_guard_output(
        &with_shared_tags(
            r#"
fn count(t out List<String>, n int = t.length())
    t = List(["new"])
    println(f"n {n}")

fn first(t out List<String>, head String = t[0])
    println(f"first {head}")
    t = List(["new"])

fn main()
    var s = Tags(tags: List<String>(["a", "b"]))
    count(s.tags)
    println(s.tags[0])
    var r = Tags(tags: List<String>(["x" + "y", "z"]))
    first(r.tags)
    println(r.tags[0])
"#,
        ),
        "n 2\nnew\nfirst xy\nnew",
    );
}

/// A default reading a scalar field taken `out` sees the field's value
/// before the call, which the callee then replaces.
#[test]
fn a_default_reads_a_scalar_field_taken_out() {
    assert_heap_guard_output(
        r#"
use system.io

struct Counter
    n int

fn step(t out int, m int = t + 1)
    println(f"m {m}")
    t = 5

fn main()
    var s = Counter(n: 41)
    step(s.n)
    println(f"{s.n}")
"#,
        "m 42\n5",
    );
}

/// A default that is the earlier `out` parameter itself holds the value
/// the place held before the call, a reference of its own: the callee
/// replacing the place first still reads that value through the default.
#[test]
fn a_default_holding_an_out_parameter_outlives_the_callee_replacing_it() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn put(c out String, d String = c)
    c = d + "!"

fn swap_first(c out String, d String = c)
    c = "x" + "y"
    println(f"d {d}")

fn main()
    var v = List(["a", "b"])
    put(v[0])
    println(v[0])
    var s = "z"
    put(s)
    println(s)
    var w = "q" + "r"
    swap_first(w)
    println(w)
    var l = List(["m" + "n"])
    swap_first(l[0])
    println(l[0])
"#,
        "a!\nz!\nd qr\nxy\nd mn\nxy",
    );
}

/// A narrow scalar handed `out` through a function value is passed as the
/// address of its slot, as the thunk the value calls declares it: a whole
/// variable and a field alike.
#[test]
fn a_function_value_takes_a_narrow_scalar_out() {
    assert_heap_guard_output(
        r#"
use system.io

struct Small
    a i8

fn bump8(x out i8)
    x = x + 1

fn main()
    var a i8 = 5
    let g = bump8
    g(a)
    var q = Small(a: 9)
    g(q.a)
    println(f"{a} {q.a}")
"#,
        "6 10",
    );
}

/// A default reading a field of a resource handed `out` reads that field
/// before the call and keeps nothing of the resource: the callee replacing
/// the place runs the old value's hook at its assignment.
#[test]
fn a_default_reading_a_field_of_an_out_resource_keeps_nothing() {
    assert_heap_guard_output(
        r#"
use system.io

class Conn
    var id int
    fn drop()
        println(f"closed {self.id}")

struct Pair
    c Conn

fn reopen(r out Conn, id int = r.id)
    r = Conn(id: id + 100)
    println(f"reopened {id}")

fn main()
    var s = Pair(c: Conn(id: 1))
    reopen(s.c)
    var c = Conn(id: 7)
    reopen(c)
    println(f"{s.c.id} {c.id}")
"#,
        "closed 1\nreopened 1\nclosed 7\nreopened 7\n101 107\nclosed 107\nclosed 101",
    );
}
