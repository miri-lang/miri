// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter's bound may name a parameter declared before it, on every
//! declaration that takes type parameters: `S implements Iterable<T>` holds the
//! collection type and its element type apart, so the body is compiled for the
//! concrete collection with no trait value in between. A parameter declared
//! after the bound is not yet in scope there.

use super::utils::*;

#[test]
fn a_function_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn first_of<T, S implements Iterable<T>>(s S) T?
    if s.length() == 0
        return None
    s.element_at(0)

fn main()
    println(first_of<String, [String]>(List(["x", "y"])) ?? "-")
"#,
        "x",
    );
}

#[test]
fn a_class_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Walker<T, S implements Iterable<T>>
    var source S
    var index int
    fn init(source S)
        self.source = source
        self.index = 0
    public fn next() T?
        if self.index >= self.source.length()
            return None
        let x = self.source.element_at(self.index)
        self.index += 1
        x

fn main()
    let w = Walker<String, [String]>(source: List(["a", "b"]))
    let a = w.next() ?? "-"
    let b = w.next() ?? "-"
    let c = w.next() ?? "-"
    println(f"{a} {b} {c}")
"#,
        "a b -",
    );
}

#[test]
fn a_struct_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct Window<T, S implements Iterable<T>>
    source S
    start int

fn head<T, S implements Iterable<T>>(w Window<T, S>) T?
    if w.start >= w.source.length()
        return None
    w.source.element_at(w.start)

fn main()
    let w = Window<int, [int]>(source: List([5, 6, 7]), start: 1)
    println(f"{head(w) ?? 0}")
"#,
        "6",
    );
}

#[test]
fn a_class_bound_may_not_name_a_later_parameter() {
    assert_compiler_error(
        r#"
use system.collections.list

class Walker<S implements Iterable<T>, T>
    var source S
    fn init(source S)
        self.source = source

fn main()
    let w = Walker<[String], String>(source: List(["a"]))
"#,
        "Unknown type: T",
    );
}

#[test]
fn an_enum_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

enum Slot<T, S implements Iterable<T>>
    Empty
    Filled(S)

fn count<T, S implements Iterable<T>>(slot Slot<T, S>) int
    match slot
        Slot.Empty: 0
        Slot.Filled(s): s.length()

fn main()
    let full = Slot<int, [int]>.Filled(List([1, 2, 3]))
    let empty = Slot<int, [int]>.Empty
    println(f"{count(full)} {count(empty)}")
"#,
        "3 0",
    );
}

#[test]
fn a_method_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Counter
    fn init()
        return
    public fn last_of<T, S implements Iterable<T>>(s S) T?
        if s.length() == 0
            return None
        s.element_at(s.length() - 1)

fn main()
    let c = Counter()
    println(c.last_of<String, [String]>(List(["p", "q"])) ?? "-")
"#,
        "q",
    );
}

#[test]
fn a_trait_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

trait Source<T, S implements Iterable<T>>
    fn items() S

class Names implements Source<String, [String]>
    var all [String]
    fn init(all [String])
        self.all = all
    public fn items() [String]
        self.all

fn main()
    let n = Names(all: List(["u", "v", "w"]))
    println(f"{n.items().length()}")
"#,
        "3",
    );
}

#[test]
fn a_type_declaration_bound_may_name_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

type Bag<T, S implements Iterable<T>> is S

fn main()
    let b Bag<int, [int]> = List<int>([1, 2])
    println(f"{b.length()}")
"#,
        "2",
    );
}
