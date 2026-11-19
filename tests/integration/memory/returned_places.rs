// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method whose body returns a place of its receiver — a field or an
//! element of `self`, an element a runtime entry point hands back out of
//! `self`, or such a call on a place of `self` — hands back a place: the
//! receiver still holds the value after the call. Keeping what such a call
//! returns at a resource type gives the resource two holders, so it is
//! refused with MER_OWN_008, as keeping `l[0]` is. A method building a new
//! value hands it over owned.

use super::super::utils::*;

const CODE: &str = "MER_OWN_008";

const PRELUDE: &str = r#"use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

class Pool
    var items List<Conn>
    fn init()
        self.items = List<Conn>()
    fn add(c Conn)
        self.items.push(c)
    fn get(i int) Conn: self.items[i]
    fn first() Conn: self.items.element_at(0)
    fn head() Conn: self.first()
    fn make(id int) Conn: Conn(id)

class Shelf
    var kept List<Conn>
    fn init()
        self.kept = List<Conn>()
    fn keep(c Conn) bool
        self.kept.push(c)
        return true
"#;

/// The program `PRELUDE` declares, then `main`.
fn program(main: &str) -> String {
    format!("{PRELUDE}{main}")
}

#[test]
fn keeping_what_an_element_getter_hands_back_is_refused() {
    let source = program(
        r#"
fn main()
    let pool = Pool()
    pool.add(Conn(1))
    let shelf = Shelf()
    shelf.keep(pool.get(0))
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "shelf.keep(pool.get(0))");
    assert!(
        diagnostic
            .message
            .contains("'pool.get(0)' is borrowed from 'pool'"),
        "the refusal names the call and its receiver: {diagnostic:#?}"
    );
    let help = diagnostic.help.unwrap_or_default();
    assert!(
        help.contains("`pool.get(0).clone()`"),
        "the repair copies the element: {help}"
    );
}

#[test]
fn keeping_a_library_element_at_is_refused_with_the_emptying_repair() {
    let source = program(
        r#"
fn main()
    var l = List<Conn>()
    l.push(Conn(1))
    let shelf = Shelf()
    shelf.keep(l.element_at(0))
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "shelf.keep(l.element_at(0))");
    let help = diagnostic.help.unwrap_or_default();
    assert!(
        help.contains("take it out instead, with `l.remove_at`"),
        "the repair names the emptying method the collection declares: {help}"
    );
}

#[test]
fn keeping_what_an_accessor_of_a_field_accessor_hands_back_is_refused() {
    let source = program(
        r#"
fn main()
    let pool = Pool()
    pool.add(Conn(1))
    let shelf = Shelf()
    shelf.keep(pool.head())
"#,
    );
    assert_refused_at(&source, CODE, "shelf.keep(pool.head())");
}

#[test]
fn a_local_bound_to_what_a_getter_hands_back_is_borrowed_too() {
    let source = program(
        r#"
fn main()
    let pool = Pool()
    pool.add(Conn(1))
    let shelf = Shelf()
    let c = pool.first()
    shelf.keep(c)
"#,
    );
    assert_refused_at(&source, CODE, "shelf.keep(c)");
}

#[test]
fn keeping_what_a_getter_builds_is_owned() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    let pool = Pool()
    let shelf = Shelf()
    shelf.keep(pool.make(4))
    println("kept")
"#,
        ),
        "kept\nclosed 4",
    );
}

#[test]
fn keeping_what_a_getter_hands_back_at_a_plain_type_is_accepted() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Ints
    var items List<int>
    fn init()
        self.items = List([4, 5])
    fn get(i int) int: self.items[i]

class IntShelf
    var kept List<int>
    fn init()
        self.kept = List<int>()
    fn keep(x int)
        self.kept.push(x)

fn main()
    let ints = Ints()
    let shelf = IntShelf()
    shelf.keep(ints.get(1))
    var l = List([7])
    shelf.keep(l.element_at(0))
    println(f"{shelf.kept.length()}")
"#,
        "2",
    );
}

/// A base whose own method hands back what its abstract `get` hands back on
/// `self`, through a local: whether a call of `grab` hands back a place is
/// each class's own `get`'s answer.
const GRAB: &str = r#"use system.collections.list

class R
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"drop {self.id}")

abstract class Base
    abstract fn get() R
    fn grab() R
        let v = self.get()
        return v

class Held extends Base
    var items List<R>
    fn init()
        self.items = List<R>()
        self.items.push(R(1))
    fn get() R: self.items[0]

class Fresh extends Base
    fn get() R: R(2)

class Shelf
    var kept List<R>
    fn init()
        self.kept = List<R>()
    fn keep(r R)
        self.kept.push(r)
"#;

#[test]
fn keeping_what_a_base_method_hands_back_is_refused_at_a_class_whose_getter_hands_back_a_place() {
    let source = format!(
        "{GRAB}\nfn main()\n    let h = Held()\n    let shelf = Shelf()\n    shelf.keep(h.grab())\n"
    );
    let diagnostic = assert_refused_at(&source, CODE, "shelf.keep(h.grab())");
    assert!(
        diagnostic
            .message
            .contains("'h.grab()' is borrowed from 'h'"),
        "the refusal names the call: {diagnostic:#?}"
    );
}

#[test]
fn keeping_what_a_base_method_hands_back_runs_at_a_class_whose_getter_builds_a_value() {
    assert_heap_guard_output(
        &format!(
            "{GRAB}\nfn main()\n    let f = Fresh()\n    let shelf = Shelf()\n    shelf.keep(f.grab())\n    println(\"kept\")\n"
        ),
        "kept\ndrop 2",
    );
}
