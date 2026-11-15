// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The calls the compiler makes where the program wrote none follow the rule
//! a written call follows. An operator and a `for` loop call a method, and a
//! method keeping a resource it is handed owns it: the caller's binding is
//! consumed, and the hook runs where the method's new owner ends. A method a
//! container's runtime thunk calls is handed borrowed elements, so one that
//! keeps a resource is refused where it is declared.

use super::super::utils::*;

const CONN: &str = r#"
use system.ops

class Conn implements Addable
    public var id int
    var peer Conn?
    fn init(id int, peer Conn?)
        self.id = id
        self.peer = peer
    fn drop(self)
        println(f"closed {self.id}")
    public fn concat(other Conn) Conn
        return Conn(self.id + 10, Some(other))
"#;

fn with_conn(program: &str) -> String {
    format!("{CONN}\n{program}")
}

/// `a + b` hands `b` to the `concat` keeping it: `b` is released with the
/// sum, which holds it.
#[test]
fn an_operand_an_operator_method_keeps_is_released_with_its_new_owner() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let b = Conn(2, None)
    if true
        let _sum = a + b
        println("built")
    println("after")
"#,
        ),
        "built\nclosed 11\nclosed 2\nafter\nclosed 1",
    );
}

/// The written call releases its kept argument at the same point the
/// operator does.
#[test]
fn a_written_concat_releases_its_kept_argument_where_the_operator_does() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let b = Conn(2, None)
    if true
        let _sum = a.concat(b)
        println("built")
    println("after")
"#,
        ),
        "built\nclosed 11\nclosed 2\nafter\nclosed 1",
    );
}

/// An operand no binding holds is handed over by the operator that made it.
#[test]
fn a_temporary_operand_an_operator_method_keeps_is_released_with_its_new_owner() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    if true
        let _sum = a + Conn(3, None)
        println("built")
    println("after")
"#,
        ),
        "built\nclosed 11\nclosed 3\nafter\nclosed 1",
    );
}

#[test]
fn an_operand_an_operator_method_keeps_cannot_be_used_after_the_operator() {
    assert_compiler_error(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let b = Conn(2, None)
    let _sum = a + b
    println(f"{b.id}")
"#,
        ),
        "'b' was consumed by 'Conn.concat' and cannot be used again",
    );
}

#[test]
fn an_operand_used_after_the_operator_keeping_it_is_reported_with_its_code() {
    assert_compiler_error(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let b = Conn(2, None)
    let _sum = a + b
    println(f"{b.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

const BAG: &str = r#"
use system.ops

class Bag implements Iterable<int>
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed bag {self.id}")
    public fn iterator() Iterator<int>
        return BagIter(Some(self), 0)

class BagIter implements Iterator<int>
    var bag Bag?
    var n int
    fn init(bag Bag?, n int)
        self.bag = bag
        self.n = n
    public fn next() int?
        if self.n < 2
            self.n = self.n + 1
            return Some(self.n)
        return None
"#;

fn with_bag(program: &str) -> String {
    format!("{BAG}\n{program}")
}

/// The loop's `iterator()` keeps the bag in the iterator it returns, so the
/// loop hands the bag over: it is released with the iterator, as the loop
/// ends, not where the bag's binding would have ended.
#[test]
fn an_iterable_its_iterator_keeps_is_released_with_the_loops_iterator() {
    assert_heap_guard_output(
        &with_bag(
            r#"
fn main()
    let bag = Bag(1)
    for x in bag
        println(f"x {x}")
    println("after loop")
"#,
        ),
        "x 1\nx 2\nclosed bag 1\nafter loop",
    );
}

/// A bag no binding holds is the loop's own temporary, handed over alike.
#[test]
fn a_temporary_iterable_its_iterator_keeps_is_released_with_the_loops_iterator() {
    assert_heap_guard_output(
        &with_bag(
            r#"
fn main()
    for x in Bag(2)
        println(f"x {x}")
    println("after loop")
"#,
        ),
        "x 1\nx 2\nclosed bag 2\nafter loop",
    );
}

#[test]
fn an_iterable_its_iterator_keeps_cannot_be_used_after_the_loop() {
    assert_compiler_error(
        &with_bag(
            r#"
fn main()
    let bag = Bag(1)
    for x in bag
        println(f"x {x}")
    println(f"{bag.id}")
"#,
        ),
        "'bag' was consumed by 'Bag.iterator' and cannot be used again",
    );
}

const HANDLE: &str = r#"
struct Shelf
    slot Handle?

class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

/// A method called `next` that no loop calls is an ordinary method: it owns
/// the resource it keeps, released where the shelf holding it ends.
#[test]
fn a_method_named_next_called_by_name_owns_what_it_keeps() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Rack
    fn init()
        println("rack")
    fn next(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("shelved")
        println("leaving next")
        return 0

fn main()
    let rack = Rack()
    let h = Handle(1)
    rack.next(h)
    println("after")
"#,
        ),
        "rack\nshelved\nclosed 1\nleaving next\nafter",
    );
}

/// A method called `concat` on a type no operator dispatches to owns what it
/// keeps as any method does.
#[test]
fn a_method_named_concat_on_a_type_without_its_operator_owns_what_it_keeps() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Rack
    fn init()
        println("rack")
    fn concat(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("shelved")
        println("leaving concat")
        return 0

fn main()
    let rack = Rack()
    rack.concat(Handle(2))
    println("after")
"#,
        ),
        "rack\nshelved\nclosed 2\nleaving concat\nafter",
    );
}

/// A set matches its elements through their `equals`, handing it borrowed
/// elements, so an `equals` keeping its parameter is refused where it is
/// declared.
#[test]
fn an_equals_a_container_calls_that_keeps_its_parameter_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        let _kept = TagShelf(slot: Some(other))
        return true

struct TagShelf
    slot Tag?

fn main()
    let a = Tag(1)
    println(f"{a.id}")
"#,
        ),
        "MER_OWN_006",
    );
}

#[test]
fn an_equals_a_container_calls_that_keeps_its_parameter_names_the_parameter() {
    assert_compiler_error(
        &with_handle(
            r#"
class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        let _kept = TagShelf(slot: Some(other))
        return true

struct TagShelf
    slot Tag?

fn main()
    let a = Tag(1)
    println(f"{a.id}")
"#,
        ),
        "'Tag.equals' keeps 'other'",
    );
}

/// A `clone` a container copies its elements through, keeping its receiver,
/// is refused alike.
#[test]
fn a_clone_a_container_calls_that_keeps_its_receiver_is_refused() {
    assert_compiler_error(
        r#"
use system.memory

class Tag implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn clone() Tag
        return self

fn main()
    let a = Tag(1)
    println(f"{a.id}")
"#,
        "MER_OWN_006",
    );
}

/// An `equals` that only reads its parameter is accepted, and so is the
/// keeping done in a method the program calls by name.
#[test]
fn an_equals_that_only_reads_its_parameter_is_accepted() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        return self.id == other.id

fn main()
    let a = Tag(1)
    let b = Tag(1)
    println(f"{a == b}")
"#,
        ),
        "true\ndropped 1\ndropped 1",
    );
}

/// A `next` can keep only its receiver, the iterator the loop holds and
/// calls `next` on again every pass, so the loop never gives the iterator
/// up: a `next` keeping it is handed a reference of its own each pass, and
/// the iterator is released once, as the loop ends.
#[test]
fn a_next_keeping_its_iterator_is_handed_a_reference_of_its_own_each_pass() {
    assert_heap_guard_output(
        r#"
use system.ops

struct Mark
    it Counter?

class Counter implements Iterator<Mark>
    var n int
    fn init()
        self.n = 0
    fn drop(self)
        println("closed counter")
    public fn next() Mark?
        if self.n < 2
            self.n = self.n + 1
            return Some(Mark(it: Some(self)))
        return None

class Source implements Iterable<Mark>
    fn init()
        println("source")
    public fn iterator() Iterator<Mark>
        return Counter()

fn main()
    for _m in Source()
        println("pass")
    println("after loop")
"#,
        "source\npass\npass\nclosed counter\nafter loop",
    );
}

/// `a + a` passes one resource to both positions of a `concat` keeping its
/// argument, as `a.concat(a)` does: a resource has one owner.
#[test]
fn an_operand_passed_to_both_sides_of_a_keeping_operator_is_refused() {
    assert_compiler_error(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let _s = a + a
    println("x")
"#,
        ),
        "'a' is passed to 'Conn.concat' more than once, and 'Conn.concat' keeps it",
    );
}

/// An operator in a loop body hands each pass's fresh operand over, and its
/// sum releases it as the pass ends.
#[test]
fn an_operator_in_a_loop_body_hands_each_passs_operand_over() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    for i in 0..2
        let _s = a + Conn(i, None)
        println(f"pass {i}")
    println("after")
"#,
        ),
        "pass 0\nclosed 11\nclosed 0\npass 1\nclosed 11\nclosed 1\nafter\nclosed 1",
    );
}

/// A binding declared outside a loop and handed over by an operator inside
/// it would be handed over again on the next pass.
#[test]
fn an_operand_from_outside_a_loop_cannot_be_handed_over_inside_it() {
    assert_compiler_error(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let b = Conn(5, None)
    for i in 0..2
        let _s = a + b
        println(f"pass {i}")
    println("after")
"#,
        ),
        "'b' was consumed by 'Conn.concat' and cannot be used again",
    );
}

/// An operator handing an operand over in one branch leaves the other
/// branch's binding to end where it was declared.
#[test]
fn an_operator_in_one_branch_hands_over_only_on_that_path() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn pick(flag bool)
    let a = Conn(1, None)
    let b = Conn(2, None)
    if flag
        let _s = a + b
        println("joined")
    println("after if")

fn main()
    pick(true)
    pick(false)
"#,
        ),
        "joined\nclosed 11\nclosed 2\nafter if\nclosed 1\nafter if\nclosed 2\nclosed 1",
    );
}

/// A binding one branch handed over may be gone after the branches join.
#[test]
fn an_operand_one_branch_hands_over_cannot_be_used_after_the_join() {
    assert_compiler_error(
        &with_conn(
            r#"
fn main()
    let a = Conn(1, None)
    let b = Conn(2, None)
    if a.id > 0
        let _s = a + b
    println(f"{b.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

const ROPE: &str = r#"
use system.ops

class Rope implements Multiplicable
    public var id int
    var src Rope?
    fn init(id int, src Rope?)
        self.id = id
        self.src = src
    fn drop(self)
        println(f"closed {self.id}")
    public fn repeat(count int) Rope
        return Rope(self.id * count, Some(self))
"#;

/// `r * 3` hands `r` to the `repeat` keeping its receiver: `r` is released
/// with the product, which holds it.
#[test]
fn a_receiver_a_repeat_keeps_is_released_with_the_product() {
    assert_heap_guard_output(
        &format!(
            "{ROPE}\n{}",
            r#"
fn main()
    let r = Rope(2, None)
    if true
        let _t = r * 3
        println("built")
    println("after")
"#
        ),
        "built\nclosed 6\nclosed 2\nafter",
    );
}

#[test]
fn a_receiver_a_repeat_keeps_cannot_be_used_after_the_operator() {
    assert_compiler_error(
        &format!(
            "{ROPE}\n{}",
            r#"
fn main()
    let r = Rope(2, None)
    let _t = r * 3
    println(f"{r.id}")
"#
        ),
        "'r' was consumed by 'Rope.repeat' and cannot be used again",
    );
}

/// A positional walk calls `element_at` on the value it walks every pass,
/// so an `element_at` keeping its receiver is handed a reference of its own
/// each pass, and the walked binding ends where it was declared.
#[test]
fn an_element_at_keeping_its_receiver_is_handed_a_reference_each_pass() {
    assert_heap_guard_output(
        r#"
use system.ops

struct Mark
    src Squares?

class Squares implements Indexed<int>
    var count int
    fn init(count int)
        self.count = count
    fn drop(self)
        println("closed squares")
    public fn length() int: self.count
    public fn element_at(index int) int
        let _kept = Mark(src: Some(self))
        return index * index

fn main()
    let sq = Squares(2)
    for x in sq
        println(f"x {x}")
    println("after loop")
"#,
        "x 0\nx 1\nafter loop\nclosed squares",
    );
}

/// A stepped range replaces its variable by the variable's `successor()`,
/// so a `successor` keeping its receiver is handed the value it replaces:
/// each day is held by the next, and all are released with the last.
#[test]
fn a_successor_keeping_its_receiver_is_handed_the_value_it_replaces() {
    assert_heap_guard_output(
        r#"
use system.ops

class Day implements Steppable
    public var n int
    var prev Day?
    fn init(n int, prev Day?)
        self.n = n
        self.prev = prev
    fn drop(self)
        println(f"closed {self.n}")
    public fn compare(other Self) int
        return self.n - other.n
    public fn successor() Self
        return Day(self.n + 1, Some(self))

fn main()
    for d in Day(1, None)..Day(3, None)
        println(f"day {d.n}")
    println("after loop")
"#,
        "day 1\nday 2\nclosed 3\nclosed 2\nclosed 1\nclosed 3\nafter loop",
    );
}

const OVERRIDDEN_ITERATOR: &str = r#"
use system.ops

class Base implements Iterable<int>
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    public fn iterator() Iterator<int>
        return Steps(None, 0)

class Sub extends Base
    fn init(id int)
        super.init(id)
    public fn iterator() Iterator<int>
        return Steps(Some(self), 0)

class Steps implements Iterator<int>
    var src Base?
    var n int
    fn init(src Base?, n int)
        self.src = src
        self.n = n
    public fn next() int?
        if self.n < 2
            self.n = self.n + 1
            return Some(self.n)
        return None
"#;

/// A loop over a base-typed binding calls `iterator()` through the vtable,
/// whose slot owns the receiver because an override keeps it: the loop
/// hands the binding over, and it is released with the loop's iterator.
#[test]
fn a_loop_hands_over_an_iterable_an_overriding_iterator_keeps() {
    assert_heap_guard_output(
        &format!(
            "{OVERRIDDEN_ITERATOR}\n{}",
            r#"
fn main()
    let b Base = Sub(1)
    for x in b
        println(f"x {x}")
    println("after loop")
"#
        ),
        "x 1\nx 2\nclosed 1\nafter loop",
    );
}

#[test]
fn an_iterable_an_overriding_iterator_keeps_cannot_be_used_after_the_loop() {
    assert_compiler_error(
        &format!(
            "{OVERRIDDEN_ITERATOR}\n{}",
            r#"
fn main()
    let b Base = Sub(1)
    for x in b
        println(f"x {x}")
    println(f"{b.id}")
"#
        ),
        "'b' was consumed by 'Base.iterator' and cannot be used again",
    );
}

/// `x in xs` is the call `xs.contains(x)`, and a list's `contains` only
/// reads what it looks for, so the value is still the caller's after it.
#[test]
fn a_value_looked_for_with_in_is_still_the_callers() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    public fn equals(other Handle) bool
        return self.id == other.id
    public fn hash() int
        return self.id

fn main()
    let xs = List<Handle>([Handle(1), Handle(2)])
    let h = Handle(2)
    println(f"{h in xs}")
    println(f"{h.id}")
    println(f"{Handle(3) in xs}")
"#,
        "true\n2\nclosed 3\nfalse\nclosed 2\nclosed 1\nclosed 2",
    );
}

/// `in` reaches only a library collection's `contains`: a class declaring
/// one is not a collection `in` searches.
#[test]
fn in_does_not_reach_a_contains_a_class_declares() {
    assert_compiler_error(
        r#"
class Bag
    fn init()
        println("bag")
    public fn contains(x int) bool
        return x == 1

fn main()
    let b = Bag()
    println(f"{1 in b}")
"#,
        "Invalid type for 'in' operator: expected collection, got Bag",
    );
}
