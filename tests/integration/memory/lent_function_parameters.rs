// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method whose body calls its function-typed parameter `f` with a value
//! it does not own whole — an element of its receiver, a `for` binding, a
//! borrowed place — lends `f` the argument at that position: the method
//! still holds the value afterwards. The function a caller passes for `f`
//! may keep none of its resource parameters at a lent position: one that
//! keeps is refused with MER_OWN_009 where it is passed. A position the
//! body hands only values it owns keeps owning them, and a method with no
//! body, or one implementing a declaration that does not lend, lends
//! nothing.

use super::super::utils::*;

const CODE: &str = "MER_OWN_009";

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
    fn count(f fn(Conn) bool) int
        var n = 0
        for c in self.items
            if f(c): n += 1
        return n

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
fn a_call_through_a_lending_parameter_lends_an_element_of_the_receiver() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    let pool = Pool()
    pool.add(Conn(1))
    pool.add(Conn(2))
    let n = pool.count(fn(c Conn) bool: c.id > 1)
    println(f"counted {n}")
    println("end")
"#,
        ),
        "counted 1\nend\nclosed 1\nclosed 2",
    );
}

#[test]
fn a_named_function_that_only_reads_is_passed_to_a_lending_parameter() {
    assert_runs_with_output(
        &program(
            r#"
fn is_first(c Conn) bool
    return c.id == 1

fn main()
    let pool = Pool()
    pool.add(Conn(1))
    pool.add(Conn(2))
    println(f"counted {pool.count(is_first)}")
"#,
        ),
        "counted 1\nclosed 1\nclosed 2",
    );
}

#[test]
fn a_lambda_keeping_its_lent_argument_is_refused_where_it_is_passed() {
    let source = program(
        r#"
fn main()
    let pool = Pool()
    let shelf = Shelf()
    pool.add(Conn(1))
    let n = pool.count(fn(c Conn) bool: shelf.keep(c))
    println(f"{n}")
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "pool.count(fn(c Conn) bool");
    assert!(
        diagnostic
            .message
            .contains("`Pool.count` lends the function passed as `f`"),
        "the refusal names the lending method: {diagnostic:#?}"
    );
}

#[test]
fn a_named_function_keeping_its_lent_argument_is_refused_where_it_is_passed() {
    let source = program(
        r#"
fn stash(c Conn) bool
    let shelf = Shelf()
    return shelf.keep(c)

fn main()
    let pool = Pool()
    pool.add(Conn(1))
    println(f"{pool.count(stash)}")
"#,
    );
    assert_refused_at(&source, CODE, "pool.count(stash)");
}

#[test]
fn a_function_value_whose_body_cannot_be_seen_is_refused_at_a_resource_parameter() {
    let source = program(
        r#"
fn tally(pool Pool, f fn(Conn) bool) int
    return pool.count(f)

fn main()
    let pool = Pool()
    pool.add(Conn(1))
    println(f"{tally(pool, fn(c Conn) bool: true)}")
"#,
    );
    assert_refused_at(&source, CODE, "return pool.count(f)");
}

#[test]
fn a_function_value_whose_body_cannot_be_seen_is_passed_at_a_plain_parameter() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Ints
    var items List<int>
    fn init()
        self.items = List([1, 2, 3])
    fn count(f fn(int) bool) int
        var n = 0
        for x in self.items
            if f(x): n += 1
        return n

fn tally(ints Ints, f fn(int) bool) int
    return ints.count(f)

fn main()
    println(f"{tally(Ints(), fn(x int) bool: x > 1)}")
"#,
        "2",
    );
}

#[test]
fn a_function_keeping_a_plain_argument_is_passed_to_a_lending_parameter() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Ints
    var items List<int>
    fn init()
        self.items = List([1, 2, 3])
    fn count(f fn(int) bool) int
        var n = 0
        for x in self.items
            if f(x): n += 1
        return n

class IntShelf
    var kept List<int>
    fn init()
        self.kept = List<int>()
    fn keep(x int) bool
        self.kept.push(x)
        return x > 1

fn stash(x int) bool
    let shelf = IntShelf()
    return shelf.keep(x)

fn main()
    println(f"{Ints().count(stash)}")
"#,
        "2",
    );
}

#[test]
fn a_library_predicate_reads_each_resource_element_it_is_lent() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    let conns = List([Conn(1), Conn(2), Conn(3)])
    let any = conns.any(fn(c Conn) bool: c.id == 2)
    let all = conns.all(fn(c Conn) bool: c.id > 0)
    let n = conns.count_where(fn(c Conn) bool: c.id > 1)
    println(f"{any} {all} {n}")
"#,
        ),
        "true true 2\nclosed 1\nclosed 2\nclosed 3",
    );
}

#[test]
fn a_library_mapping_keeping_its_lent_element_is_refused_where_it_is_passed() {
    let source = program(
        r#"
fn main()
    let conns = List([Conn(1), Conn(2)])
    let same = conns.map(fn(c Conn) Conn: c)
    println(f"{same.length()}")
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "conns.map(fn(c Conn) Conn: c)");
    assert!(
        diagnostic
            .message
            .contains("lends the function passed as `f`"),
        "the refusal names the lent parameter: {diagnostic:#?}"
    );
}

#[test]
fn a_library_mapping_building_new_resources_runs_every_hook_once() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    let conns = List([Conn(1), Conn(2)])
    let next = conns.map(fn(c Conn) Conn: Conn(c.id + 10))
    println(f"{next.length()} {next[1].id}")
"#,
        ),
        "2 12\nclosed 11\nclosed 12\nclosed 1\nclosed 2",
    );
}

#[test]
fn a_method_handing_its_function_an_element_by_index_lends_it() {
    let source = r#"
use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

class Row
    var items List<Conn>
    fn init()
        self.items = List([Conn(1), Conn(2)])
    fn any(f fn(Conn) bool) bool
        var i = 0
        while i < self.items.length()
            if f(self.items.element_at(i)): return true
            i += 1
        return false

class Shelf
    var kept List<Conn>
    fn init()
        self.kept = List<Conn>()
    fn keep(c Conn) bool
        self.kept.push(c)
        return true

fn main()
    let row = Row()
    let shelf = Shelf()
    println(f"{row.any(fn(c Conn) bool: shelf.keep(c))}")
"#;
    assert_refused_at(source, CODE, "row.any(fn(c Conn) bool");
}

#[test]
fn a_method_handing_its_function_only_owned_values_owns_them() {
    assert_runs_with_output(
        &program(
            r#"
class Maker
    var next int
    fn init()
        self.next = 7
    fn each(f fn(Conn) bool) bool
        return f(Conn(self.next))

fn main()
    let shelf = Shelf()
    let kept = Maker().each(fn(c Conn) bool: shelf.keep(c))
    println(f"{kept} {shelf.kept.length()}")
"#,
        ),
        "true 1\nclosed 7",
    );
}

#[test]
fn an_implementation_of_an_abstract_method_handing_its_function_an_element_is_refused() {
    let source = r#"
use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

trait Visits
    abstract fn visit(f fn(Conn) bool) int

class Pool implements Visits
    var items List<Conn>
    fn init()
        self.items = List([Conn(1)])
    fn visit(f fn(Conn) bool) int
        var n = 0
        for c in self.items
            if f(c): n += 1
        return n

fn main()
    println(f"{Pool().visit(fn(c Conn) bool: c.id > 0)}")
"#;
    let diagnostic = assert_refused_at(source, "MER_OWN_008", "if f(c): n += 1");
    assert!(
        diagnostic
            .message
            .contains("'c' is borrowed from 'self.items'"),
        "the override hands the element on owned: {diagnostic:#?}"
    );
}

#[test]
fn a_call_through_a_trait_typed_receiver_is_checked_against_the_default() {
    let source = r#"
use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

trait Walk
    abstract fn at(i int) Conn
    fn apply(f fn(Conn) bool) bool
        return f(self.at(0))

class Pool implements Walk
    var items List<Conn>
    fn init()
        self.items = List([Conn(1)])
    fn at(i int) Conn: self.items[i]

class Shelf
    var kept List<Conn>
    fn init()
        self.kept = List<Conn>()
    fn keep(c Conn) bool
        self.kept.push(c)
        return true

fn walk(w Walk, shelf Shelf) bool
    return w.apply(fn(c Conn) bool: shelf.keep(c))

fn main()
    println(f"{walk(Pool(), Shelf())}")
"#;
    assert_refused_at(source, CODE, "w.apply(fn(c Conn) bool");
}

#[test]
fn a_default_handing_its_function_what_an_owned_getter_builds_owns_it_at_that_class() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

trait Walk
    abstract fn at(i int) Conn
    fn apply(f fn(Conn) bool) bool
        return f(self.at(0))

class Fresh implements Walk
    fn init()
        let unused = 0
    fn at(i int) Conn: Conn(i + 5)

class Shelf
    var kept List<Conn>
    fn init()
        self.kept = List<Conn>()
    fn keep(c Conn) bool
        self.kept.push(c)
        return true

fn main()
    let shelf = Shelf()
    println(f"{Fresh().apply(fn(c Conn) bool: shelf.keep(c))} {shelf.kept.length()}")
"#,
        "true 1\nclosed 5",
    );
}

#[test]
fn a_lambda_parameter_shadowing_a_lending_parameter_is_not_lent() {
    let source = program(
        r#"
class Probe
    var items List<Conn>
    fn init()
        self.items = List([Conn(1)])
    fn check(f fn(Conn) bool) bool
        let inner = fn(f fn(Conn) bool) bool: f(self.items[0])
        return inner(f)

fn main()
    println(f"{Probe().check(fn(c Conn) bool: c.id > 0)}")
"#,
    );
    assert_refused_at(
        &source,
        "MER_OWN_008",
        "fn(f fn(Conn) bool) bool: f(self.items[0])",
    );
}

#[test]
fn a_loop_binding_shadowing_a_lending_parameter_is_not_lent() {
    let source = program(
        r#"
class Probe
    var items List<Conn>
    var checks List<fn(Conn) bool>
    fn init()
        self.items = List([Conn(1)])
        self.checks = List<fn(Conn) bool>()
    fn check(f fn(Conn) bool) int
        var n = 0
        for f in self.checks
            if f(self.items[0]): n += 1
        return n

fn main()
    println(f"{Probe().check(fn(c Conn) bool: c.id > 0)}")
"#,
    );
    assert_refused_at(&source, "MER_OWN_008", "if f(self.items[0]): n += 1");
}

#[test]
fn a_reducer_returning_its_accumulator_runs_at_a_resource() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    let conns = List([Conn(1), Conn(2)])
    let first = conns.reduce(Conn(0), fn(acc Conn, c Conn) Conn: acc)
    let total = conns.reduce(Conn(0), fn(acc Conn, c Conn) Conn: Conn(acc.id + c.id))
    println(f"{first.id} {total.id}")
"#,
        ),
        "closed 1\nclosed 0\n0 3\nclosed 3\nclosed 0\nclosed 1\nclosed 2",
    );
}

#[test]
fn a_reducer_keeping_the_element_it_is_lent_is_refused() {
    let source = program(
        r#"
fn main()
    let conns = List([Conn(1), Conn(2)])
    let last = conns.reduce(Conn(0), fn(acc Conn, c Conn) Conn: c)
    println(f"{last.id}")
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "fn(acc Conn, c Conn) Conn: c");
    assert!(
        diagnostic.message.contains("its argument 2"),
        "only the element is lent: {diagnostic:#?}"
    );
}

#[test]
fn a_generic_function_mapping_by_identity_is_refused_at_a_resource() {
    let source = program(
        r#"
fn keep_all<T>(l [T]) [T]
    return l.map(fn(x T) T: x)

fn main()
    let conns = List([Conn(1)])
    println(f"{keep_all(conns).length()}")
"#,
    );
    assert_refused_at(&source, CODE, "l.map(fn(x T) T: x)");
}

#[test]
fn a_generic_function_mapping_by_identity_runs_at_a_plain_type() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn keep_all<T>(l [T]) [T]
    return l.map(fn(x T) T: x)

fn main()
    println(f"{keep_all(List([1, 2])).length()}")
"#,
        "2",
    );
}

/// A base handing its function what its abstract `get` hands back on
/// `self`, then returning it: whether the function is lent the value is each
/// class's own `get`'s answer, and the body is one for every class.
fn each_program(classes: &str, main: &str) -> String {
    format!(
        r#"use system.collections.list

class R
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"drop {{self.id}}")

abstract class Base
    abstract fn get() R
    fn each(f fn(R)) R
        let v = self.get()
        f(v)
        return v

class Shelf
    var kept List<R>
    fn init()
        self.kept = List<R>()
    fn keep(r R)
        self.kept.push(r)
{classes}{main}"#
    )
}

const HELD: &str = r#"
class Held extends Base
    var items List<R>
    fn init()
        self.items = List<R>()
        self.items.push(R(1))
    fn get() R: self.items[0]
"#;

const FRESH: &str = r#"
class Fresh extends Base
    fn get() R: R(2)
"#;

#[test]
fn a_body_lends_what_every_class_running_it_hands_back_from_a_place() {
    let source = each_program(
        HELD,
        "\nfn main()\n    let h = Held()\n    let shelf = Shelf()\n    let r = h.each(fn(x R): shelf.keep(x))\n    println(f\"{r.id}\")\n",
    );
    assert_refused_at(&source, CODE, "let r = h.each(");
}

#[test]
fn a_function_reading_what_every_class_lends_runs() {
    assert_heap_guard_output(
        &each_program(
            HELD,
            "\nfn main()\n    let h = Held()\n    let r = h.each(fn(x R): println(f\"saw {x.id}\"))\n    println(f\"got {r.id}\")\n",
        ),
        "saw 1\ngot 1\ndrop 1",
    );
}

/// At a class whose getter builds a value a caller may hand a keeper, so
/// the body, one for every class, cannot treat its function as lent: the
/// value handed on is refused there, wherever its place may be.
#[test]
fn a_body_handing_a_value_some_class_owns_to_its_function_is_refused_in_the_body() {
    let source = each_program(
        &format!("{HELD}{FRESH}"),
        "\nfn main()\n    let f = Fresh()\n    let shelf = Shelf()\n    let r = f.each(fn(x R): shelf.keep(x))\n    println(f\"{r.id}\")\n",
    );
    assert_refused_at(&source, "MER_OWN_008", "        f(v)");
}
