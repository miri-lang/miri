// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A managed value read out of a list, struct, tuple, enum or optional a body
//! owns borrows it, taking no reference, while nothing can change or release
//! what it was read from. Each program is checked for its output under the
//! leak check and under the heap guard, and for exactly how many references
//! it took and released.

use super::super::utils::{assert_heap_guard_output, assert_runs_with_output};
use super::rc_stats::rc_stats;

/// Run `code` under the leak check and under the heap guard, expecting
/// `expected` both times, and expect `dup` retains and `drop` releases.
fn assert_borrowing(code: &str, expected: &str, dup: usize, drop: usize) {
    assert_runs_with_output(code, expected);
    assert_heap_guard_output(code, expected);
    let line = rc_stats(code, &[]);
    let counts = format!("dup={dup} drop={drop} ");
    assert!(line.contains(&counts), "expected {counts}in {line}");
}

/// `match code[pc]` reads its subject out of a list the body owns and does
/// not change while the arms run, so the subject borrows the element: the four
/// matches executed retain nothing. The variant carrying a string, never
/// built, keeps `Instr` a counted allocation.
#[test]
fn test_match_on_an_element_of_a_local_list_takes_no_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

enum Instr
    Add(int)
    Twice
    Halt
    Note(String)

fn run() int
    let code = List([Instr.Add(3), Instr.Twice, Instr.Add(4), Instr.Halt])
    var acc = 0
    var pc = 0
    var running = true
    while running
        match code[pc]
            Instr.Add(v): acc = acc + v
            Instr.Twice: acc = acc * 2
            Instr.Halt: running = false
            Instr.Note(_): acc = acc
        pc += 1
    acc

println(f"{run()}")
"#,
        "10",
        4,
        15,
    );
}

/// An enum of scalar payloads is held as its bytes inside the list, so
/// building, matching and releasing its elements counts nothing: what is
/// left is the list and the formatted output.
#[test]
fn test_match_on_an_element_held_as_its_bytes_counts_nothing() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

enum Instr
    Add(int)
    Twice
    Halt

fn run() int
    let code = List([Instr.Add(3), Instr.Twice, Instr.Add(4), Instr.Halt])
    var acc = 0
    var pc = 0
    var running = true
    while running
        match code[pc]
            Instr.Add(v): acc = acc + v
            Instr.Twice: acc = acc * 2
            Instr.Halt: running = false
        pc += 1
    acc

println(f"{run()}")
"#,
        "10",
        0,
        3,
    );
}

/// `for s in shapes` over a list the body owns borrows each element for the
/// iteration, virtual call included: only the iteration's own copy of the list
/// and the shapes' construction are counted.
#[test]
fn test_loop_variable_over_a_local_list_takes_no_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

trait Shape
    fn area() int

class Square implements Shape
    var side int

    fn init(side int)
        self.side = side

    public fn area() int: self.side * self.side

class Rect implements Shape
    var w int
    var h int

    fn init(w int, h int)
        self.w = w
        self.h = h

    public fn area() int: self.w * self.h

fn total() int
    var shapes = List<Shape>()
    shapes.push(Square(2))
    shapes.push(Rect(2, 3))
    shapes.push(Square(3))
    var t = 0
    var round = 0
    while round < 2
        for s in shapes
            t = t + s.area()
        round += 1
    t

println(f"{total()}")
"#,
        "38",
        5,
        10,
    );
}

/// Clearing, growing or storing into the list an element was read from may
/// free the element, so a read held across any of them keeps its reference:
/// the counts are those of a program with no borrowing at all.
#[test]
fn test_reads_before_a_clear_a_push_or_an_index_store_keep_their_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

class Box
    var v int

    fn init(v int)
        self.v = v

fn read_after_clear() int
    var xs = List([Box(1), Box(2)])
    let b = xs[0]
    xs.clear()
    b.v

fn read_after_push() int
    var xs = List([Box(1), Box(2)])
    let b = xs[1]
    var i = 0
    while i < 40
        xs.push(Box(i))
        i += 1
    b.v + xs.length()

fn read_after_index_assign() int
    var xs = List([Box(1), Box(2)])
    let b = xs[0]
    xs[0] = Box(9)
    b.v * 10 + xs[0].v

println(f"{read_after_clear()} {read_after_push()} {read_after_index_assign()}")
"#,
        "1 44 19",
        50,
        112,
    );
}

/// A borrowed element escaping into a list, a return slot, a closure or an
/// owned parameter takes its own reference there, and outlives the list it
/// was read from. The three borrows save one retain each; the owned argument
/// is funded either way.
#[test]
fn test_an_element_stored_returned_captured_or_passed_owned_is_funded() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

class Box
    var v int

    fn init(v int)
        self.v = v

class Wrapper
    var inner Box

    fn init(inner Box)
        self.inner = inner

fn wrap(b Box) Wrapper
    var kept = b
    Wrapper(kept)

fn stored() [Box]
    let xs = List([Box(1), Box(2)])
    var ys = List<Box>()
    let b = xs[0]
    ys.push(b)
    ys

fn returned() Box
    let xs = List([Box(3), Box(4)])
    let b = xs[1]
    b

fn captured() fn() int
    let xs = List([Box(5), Box(6)])
    let b = xs[0]
    fn() int: b.v

fn passed_owned() Wrapper
    let xs = List([Box(7), Box(8)])
    let b = xs[1]
    wrap(b)

let ys = stored()
let r = returned()
let f = captured()
let w = passed_owned()
println(f"{ys[0].v} {r.v} {f()} {w.inner.v}")
"#,
        "1 4 5 8",
        14,
        45,
    );
}

/// The arm stores into and then replaces the list the subject was read
/// from, so the subject keeps its reference; the string bound out of it is
/// still borrowed from the subject, which holds it until the arms join. The
/// string is a literal, whose retain is not counted, so only its two
/// releases go.
#[test]
fn test_a_match_subject_whose_list_the_arm_replaces_keeps_its_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

enum Op
    Push(String)
    Stop

fn run() String
    var code = List([Op.Push("a"), Op.Push("b"), Op.Stop])
    var out = ""
    var pc = 0
    var going = true
    while going
        match code[pc]
            Op.Push(s)
                code[pc] = Op.Stop
                code = List([Op.Push("x"), Op.Push("y"), Op.Stop])
                out = out + s
                going = true
            Op.Stop: going = false
        pc += 1
    out

println(run())
"#,
        "ay",
        17,
        50,
    );
}

/// Each loop iterates over a copy of its own, so neither loop variable's
/// list changes under it: the three outer and nine inner reads retain
/// nothing.
#[test]
fn test_nested_loops_over_one_list_borrow_both_loop_variables() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

class Box
    var v int

    fn init(v int)
        self.v = v

fn pairs() int
    let xs = List([Box(1), Box(2), Box(3)])
    var t = 0
    for a in xs
        for b in xs
            t = t + a.v * b.v
    t

println(f"{pairs()}")
"#,
        "36",
        7,
        16,
    );
}

/// `row.push` asks whether `row` is shared before writing. A borrowed `row`
/// would answer no and grow the list inside `grid`; it keeps its reference,
/// so the write copies and `grid[0]` is unchanged.
#[test]
fn test_a_written_element_is_copied_and_leaves_its_list_alone() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

fn grow() String
    var grid = List([List([1, 2]), List([3])])
    let row = grid[0]
    row.push(9)
    f"{grid[0].length()} {row.length()} {grid[0][0]}"

println(grow())
"#,
        "2 3 1",
        4,
        15,
    );
}

/// `h.wipe()` empties the list `xs` names without mentioning `xs`, so an
/// element read from a parameter keeps its reference. The caller retains
/// `h.items` for the call, since `probe` may release what that field holds,
/// so `wipe` copies the list before clearing it and `xs` keeps both nodes.
#[test]
fn test_an_element_of_a_borrowed_parameter_keeps_its_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

class Node
    var v int

    fn init(v int)
        self.v = v

class Holder
    var items [Node]

    fn init(items [Node])
        self.items = items

    fn wipe()
        self.items.clear()

fn probe(h Holder, xs [Node]) int
    let x = xs[0]
    h.wipe()
    x.v

let h = Holder(List([Node(3), Node(4)]))
println(f"{probe(h, h.items)}")
"#,
        "3",
        5,
        16,
    );
}

/// `node.cut()` clears the field the subject was read from without naming
/// it, so `node.left` keeps its reference. The payload bound out of it is
/// borrowed from the subject, which holds it.
#[test]
fn test_a_class_field_read_keeps_its_reference() {
    assert_borrowing(
        r#"
use system.io

class Node
    var left Node?
    var v int

    fn init(left Node?, v int)
        self.left = left
        self.v = v

    fn cut()
        self.left = None

fn probe() int
    let node = Node(Node(None, 7), 1)
    match node.left
        Some(l)
            node.cut()
            l.v
        None: 0

println(f"{probe()}")
"#,
        "7",
        3,
        6,
    );
}

/// Replacing `h` runs its drop hook, which clears a list it shares with
/// `xs`; the clear copies first, so the list the loop reads is untouched and
/// both loop variables borrow.
#[test]
fn test_a_drop_hook_clearing_a_held_list_leaves_the_loop_variable_valid() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

class Box
    var v int

    fn init(v int)
        self.v = v

class Holder
    var items [Box]

    fn init(items [Box])
        self.items = items

    fn drop()
        self.items.clear()

fn total() int
    var xs = List([Box(1), Box(2)])
    var h = Holder(xs)
    var t = 0
    for s in xs
        h = Holder(List<Box>())
        t = t + s.v
    t + h.items.length()

println(f"{total()}")
"#,
        "3",
        10,
        25,
    );
}

/// `for row in grid` lends each row to `sum`, a function of the program that
/// reads its parameter in place: the row borrows `grid`'s element, so the
/// three iterations retain and release nothing.
#[test]
fn test_a_loop_variable_lent_to_a_function_takes_no_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

fn sum(xs [int]) int
    var t = 0
    var i = 0
    while i < xs.length()
        t = t + xs[i]
        i += 1
    t

fn run() int
    var grid = List([List([1, 2]), List([3, 4]), List([5, 6])])
    var total = 0
    for row in grid
        total = total + sum(row)
    total

println(f"{run()}")
"#,
        "21",
        4,
        16,
    );
}

/// `grown` pushes to and reverses the list it is lent, and keeps it lent: a
/// list of trait values may run a drop hook when released, so the parameter
/// is never taken over. It binds a copy of the parameter on entry, which
/// takes a reference of its own, so each of the two calls copies the
/// borrowed `rows.first` before writing and `rows` keeps its one element.
#[test]
fn test_a_function_writing_a_borrowed_element_leaves_its_list_alone() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

trait Shape
    fn area() int

class Square implements Shape
    var side int

    fn init(side int)
        self.side = side

    public fn area() int: self.side * self.side

struct Rows
    first [Shape]
    second [Shape]

fn grown(xs [Shape]) int
    xs.push(Square(3))
    xs.reverse()
    var t = 0
    for s in xs
        t = t + s.area()
    t + xs.length()

fn run() String
    let a = List<Shape>()
    a.push(Square(1))
    let b = List<Shape>()
    b.push(Square(2))
    let rows = Rows(a, b)
    var total = 0
    var i = 0
    while i < 2
        let row = rows.first
        total = total + grown(row)
        i += 1
    f"{total} {rows.first.length()} {rows.first[0].area()}"

println(run())
"#,
        "24 1 1",
        11,
        23,
    );
}

/// A method's receiver is written in place, relying on its caller's
/// copy-on-write or a check that it arrived unique, which a borrowed row
/// would answer with its owner's count. A row a method is called on — one
/// that only reads it, or `sort`, which writes it — keeps its reference.
#[test]
fn test_an_element_a_method_is_called_on_keeps_its_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

fn run() String
    var grid = List([List([1, 2]), List([3, 4]), List([5, 6])])
    var found = 0
    for row in grid
        if row.contains(4)
            found += 1
    var sorted = List([List([2, 1]), List([4, 3])])
    var firsts = 0
    for row in sorted
        row.sort()
        firsts = firsts + row[0]
    f"{found} {firsts} {sorted[0][0]} {sorted[1][0]}"

println(run())
"#,
        "1 4 2 4",
        13,
        36,
    );
}

/// A string, an optional and a closure's argument are not copied on entry by
/// the callee writing them, so each keeps its reference when lent: the
/// counts are those of a program with no borrowing of them.
#[test]
fn test_a_string_an_optional_or_a_closure_argument_keeps_its_reference() {
    assert_borrowing(
        r#"
use system.io
use system.collections.list

fn width(s String) int: s.length()

fn present(b Box?) int
    match b
        Some(x): x.v
        None: 0

class Box
    var v int

    fn init(v int)
        self.v = v

fn run() String
    let names = List(["ab", "cde", "f"])
    var w = 0
    for s in names
        w = w + width(s)
    let boxes = List<Box?>()
    boxes.push(Box(4))
    boxes.push(None)
    boxes.push(Box(5))
    var p = 0
    for b in boxes
        p = p + present(b)
    let grid = List([List([1, 2]), List([3])])
    let count = fn(xs [int]) int: xs.length()
    var c = 0
    for row in grid
        c = c + count(row)
    f"{w} {p} {c}"

println(run())
"#,
        "6 9 3",
        14,
        40,
    );
}

/// Run `code` under the leak check and under the heap guard, expecting
/// `expected` both times.
fn assert_correct(code: &str, expected: &str) {
    assert_runs_with_output(code, expected);
    assert_heap_guard_output(code, expected);
}

/// A callee writes a parameter in place when nothing else holds it, so
/// lending the list an element was read from to `mutate` may empty it: the
/// element keeps its reference.
#[test]
fn test_an_element_of_a_list_lent_to_a_callee_that_clears_it_survives() {
    assert_correct(
        r#"
use system.io
use system.collections.list
use system.collections.array

class Box
    var v int

    fn init(v int)
        self.v = v
fn mutate(xs [Box]) int
    xs.clear()
    xs.length()
fn t() String
    var xs = List([Box(1), Box(2)])
    let b = xs[0]
    let n = mutate(xs)
    let c = Box(99)
    f"{b.v} {c.v}"
println(t())
"#,
        "1 99",
    );
}

/// The same through a struct root: `eat` clears the list inside the struct
/// it is lent.
#[test]
fn test_an_element_of_a_struct_lent_to_a_callee_that_clears_it_survives() {
    assert_correct(
        r#"
use system.io
use system.collections.list
use system.collections.array

class Box
    var v int

    fn init(v int)
        self.v = v
struct S
    items [Box]
fn eat(s S) int
    s.items.clear()
    0
fn t() int
    let s = S(List([Box(1), Box(2)]))
    let b = s.items[0]
    eat(s)
    b.v
println(f"{t()}")
"#,
        "1",
    );
}
