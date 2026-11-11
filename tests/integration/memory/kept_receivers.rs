// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method that keeps its resource receiver, or another resource parameter,
//! owns it: the caller's reference transfers at the call, and the drop hook
//! runs where the method's new owner ends, not after the call returns. Every
//! test reads the exact order of the hook's output under the heap guard.

use super::super::utils::*;

const HANDLE: &str = r#"
struct Shelf
    slot Handle?

class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn put_on(self, n int) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_on")
        return n
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

/// The receiver ends with the shelf the method builds from it, inside the
/// method, before the method prints that it is leaving.
#[test]
fn a_kept_receiver_is_released_where_the_methods_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    h.put_on(2)
    println("after")
"#,
        ),
        "on shelf\nclosed 1\nleaving put_on\nafter",
    );
}

/// A receiver no binding holds is handed over by the call that made it.
#[test]
fn a_temporary_kept_receiver_is_released_where_the_methods_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    Handle(1).put_on(2)
    println("after")
"#,
        ),
        "on shelf\nclosed 1\nleaving put_on\nafter",
    );
}

/// The receiver another call returns is that call's temporary, handed over.
#[test]
fn a_chained_kept_receiver_is_released_where_the_methods_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Maker
    fn init()
        println("maker")
    fn make(id int) Handle
        return Handle(id)

fn main()
    Maker().make(3).put_on(4)
    println("after")
"#,
        ),
        "maker\non shelf\nclosed 3\nleaving put_on\nafter",
    );
}

/// A method a derived class inherits, and no class overrides, is called
/// directly; the instance it keeps is released as its own class, so the
/// derived hook runs where the method's new owner ends.
#[test]
fn an_inherited_kept_receiver_runs_the_derived_hook_where_the_new_owner_ends() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed base {self.id}")
    fn put_on(self) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving")
        return 0

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed derived {self.id}")

fn main()
    let d = Derived(3)
    d.put_on()
    println("after")
"#,
        "on shelf\nclosed derived 3\nleaving\nafter",
    );
}

/// A method a subclass overrides takes a vtable slot, so whichever body
/// answers is reached by one calling convention, and that convention lends
/// the receiver: the caller releases it after the call returns, here after
/// the overriding body runs on a derived instance.
#[test]
fn an_overridden_keeper_lends_its_receiver_and_the_caller_releases_it() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn put_on(self, n int) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_on")
        return n

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn put_on(self, n int) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("derived on shelf")
        println("leaving derived put_on")
        return n

fn main()
    let b = Base(1)
    b.put_on(2)
    println("after base")
    let d = Derived(2)
    d.put_on(3)
    println("after derived")
"#,
        "on shelf\nleaving put_on\nclosed 1\nafter base\nderived on shelf\nleaving derived put_on\nclosed 2\nafter derived",
    );
}

/// A method a trait declares takes a vtable slot too, and lends its receiver
/// for the same reason, even on a class nothing extends.
#[test]
fn a_trait_declared_keeper_lends_its_receiver_and_the_caller_releases_it() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Base?

trait Shelvable
    fn stash(n int) int

class Base implements Shelvable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn stash(n int) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("stashed")
        println("leaving stash")
        return n

fn main()
    let c = Base(2)
    c.stash(3)
    println("after stash")
"#,
        "stashed\nleaving stash\nclosed 2\nafter stash",
    );
}

/// A `var` handed to a keeping method holds nothing until it is assigned
/// again, so the assignment releases nothing and each hook runs once.
#[test]
fn a_var_reassigned_after_a_kept_call_runs_each_hook_once() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    var h = Handle(1)
    h.put_on(2)
    h = Handle(2)
    println("reassigned")
"#,
        ),
        "on shelf\nclosed 1\nleaving put_on\nreassigned\nclosed 2",
    );
}

/// A receiver read out of a field is funded for the method with a reference
/// of its own: the method releases that one, and the field keeps the value
/// until its holder goes.
#[test]
fn a_field_receiver_is_funded_and_its_holder_still_releases_it() {
    assert_heap_guard_output(
        &with_handle(
            r#"
struct Box
    h Handle

fn main()
    let b = Box(h: Handle(5))
    b.h.put_on(2)
    println("after")
"#,
        ),
        "on shelf\nleaving put_on\nafter\nclosed 5",
    );
}

/// The escape analysis summarizes no enum method, so no enum method keeps its
/// receiver yet: the caller still releases it after the call.
#[test]
fn a_resource_enum_receiver_is_released_by_its_caller() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Res?

enum Res
    A(int)
    B

    fn drop(self)
        println("closed res")

    fn put_on(self) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving")
        return 0

fn main()
    let r = Res.A(1)
    r.put_on()
    println("after")
"#,
        "on shelf\nleaving\nafter\nclosed res",
    );
}

#[test]
fn a_kept_receiver_cannot_be_used_after_the_call() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    h.put_on(2)
    h.put_on(3)
"#,
        ),
        "'h' was consumed by 'Handle.put_on' and cannot be used again",
    );
}

#[test]
fn a_kept_receiver_used_after_the_call_is_reported_with_its_code() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    h.put_on(2)
    h.put_on(3)
"#,
        ),
        "MER_OWN_003",
    );
}

/// An `out` parameter is the caller's binding: the method is funded with a
/// reference of its own, and the caller's binding keeps the value.
#[test]
fn an_out_parameter_used_as_a_kept_receiver_stays_the_callers() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn take(h out Handle)
    h.put_on(2)

fn main()
    var h = Handle(5)
    take(h)
    println("after")
"#,
        ),
        "on shelf\nleaving put_on\nafter\nclosed 5",
    );
}

/// A method keeping two of its resource parameters owns both: each is
/// released where its own new owner ends, in reverse order of the shelves.
#[test]
fn a_method_keeping_two_parameters_releases_each_where_its_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Rack
    fn init()
        println("rack")
    fn put(a Handle, b Handle) int
        if true
            let _first = Shelf(slot: Some(a))
            if true
                let _second = Shelf(slot: Some(b))
                println("racked both")
            println("racked one")
        println("leaving put")
        return 0

fn main()
    let rack = Rack()
    let h = Handle(1)
    rack.put(h, Handle(2))
    println("after")
"#,
        ),
        "rack\nracked both\nclosed 2\nracked one\nclosed 1\nleaving put\nafter",
    );
}

/// A kept receiver and a kept parameter are both the method's, released in
/// reverse order of the shelves that hold them.
#[test]
fn a_kept_receiver_and_a_kept_parameter_run_both_hooks_in_order() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Twin
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed twin {self.id}")
    fn join(self, other Handle) int
        if true
            let _mine = TwinShelf(slot: Some(self))
            let _theirs = Shelf(slot: Some(other))
            println("joined")
        println("leaving join")
        return 0

struct TwinShelf
    slot Twin?

fn main()
    let t = Twin(1)
    let h = Handle(2)
    t.join(h)
    println("after")
"#,
        ),
        "joined\nclosed 2\nclosed twin 1\nleaving join\nafter",
    );
}

/// A static method keeping a resource parameter owns it as a function does.
#[test]
fn a_static_methods_kept_parameter_is_released_where_its_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Store
    static fn shelve(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("shelved")
        println("leaving shelve")
        return 0

fn main()
    let h = Handle(1)
    Store.shelve(h)
    Store.shelve(Handle(2))
    println("after")
"#,
        ),
        "shelved\nclosed 1\nleaving shelve\nshelved\nclosed 2\nleaving shelve\nafter",
    );
}

#[test]
fn a_static_methods_kept_parameter_cannot_be_used_after_the_call() {
    assert_compiler_error(
        &with_handle(
            r#"
class Store
    static fn shelve(h Handle) int
        let _shelf = Shelf(slot: Some(h))
        return 0

fn main()
    let h = Handle(1)
    Store.shelve(h)
    Store.shelve(h)
"#,
        ),
        "'h' was consumed by 'shelve' and cannot be used again",
    );
}

#[test]
fn a_static_methods_kept_parameter_used_after_the_call_is_reported_with_its_code() {
    assert_compiler_error(
        &with_handle(
            r#"
class Store
    static fn shelve(h Handle) int
        let _shelf = Shelf(slot: Some(h))
        return 0

fn main()
    let h = Handle(1)
    Store.shelve(h)
    Store.shelve(h)
"#,
        ),
        "MER_OWN_003",
    );
}

/// Methods the run-time and early-return tests share, written inside the
/// `Handle` class after `put_on`.
const GATED_METHODS: &str = r#"
    fn put_if(self, n int) int
        if n > 0
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_if")
        return n
    fn put_or_leave(self, n int) int
        if n == 0
            println("left early")
            return 0
        if true
            let _shelf = Shelf(slot: Some(self))
            println("kept")
        println("leaving put_or_leave")
        return n
    fn put_then_return(self, n int) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
            if n > 0
                println("returning")
                return n
        println("leaving put_then_return")
        return 0
    fn peek() int
        return self.id
"#;

fn with_gated_handle(program: &str) -> String {
    with_handle(&format!("{GATED_METHODS}\n{program}"))
}

/// A method that keeps its receiver on one path only owns it on every path:
/// where it keeps it the hook runs where the shelf ends, and where it does
/// not the method releases it when it returns.
#[test]
fn a_receiver_kept_on_one_path_is_released_by_the_method_on_the_other() {
    assert_heap_guard_output(
        &with_gated_handle(
            r#"
fn main()
    let h = Handle(1)
    h.put_if(0)
    println("after first")
    Handle(2).put_if(1)
    println("after second")
"#,
        ),
        "leaving put_if\nclosed 1\nafter first\non shelf\nclosed 2\nleaving put_if\nafter second",
    );
}

/// A return before the keep releases the receiver at that return; the path
/// that keeps it releases it where the shelf ends.
#[test]
fn a_receiver_returned_past_before_its_keep_is_released_at_the_return() {
    assert_heap_guard_output(
        &with_gated_handle(
            r#"
fn main()
    let h = Handle(1)
    h.put_or_leave(0)
    println("after first")
    Handle(2).put_or_leave(1)
    println("after second")
"#,
        ),
        "left early\nclosed 1\nafter first\nkept\nclosed 2\nleaving put_or_leave\nafter second",
    );
}

/// A return inside the shelf's scope ends the shelf there, and with it the
/// receiver; the path that falls through ends it with the block.
#[test]
fn a_receiver_kept_before_a_return_is_released_at_the_return() {
    assert_heap_guard_output(
        &with_gated_handle(
            r#"
fn main()
    Handle(1).put_then_return(1)
    println("after first")
    Handle(2).put_then_return(0)
    println("after second")
"#,
        ),
        "on shelf\nreturning\nclosed 1\nafter first\non shelf\nclosed 2\nleaving put_then_return\nafter second",
    );
}

/// Each iteration's receiver is handed over by the call that made it and
/// released inside the method, once per iteration.
#[test]
fn a_receiver_kept_in_a_loop_is_released_once_per_iteration() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    for i in 0..3
        Handle(i).put_on(i)
    println("after")
"#,
        ),
        "on shelf\nclosed 0\nleaving put_on\non shelf\nclosed 1\nleaving put_on\non shelf\nclosed 2\nleaving put_on\nafter",
    );
}

/// A loop whose method keeps the receiver on odd iterations only: the method
/// releases it when it returns on the others.
#[test]
fn a_receiver_kept_on_odd_iterations_is_released_by_the_method_on_even_ones() {
    assert_heap_guard_output(
        &with_gated_handle(
            r#"
fn main()
    for i in 0..3
        let h = Handle(i)
        h.put_if(i % 2)
    println("after")
"#,
        ),
        "leaving put_if\nclosed 0\non shelf\nclosed 1\nleaving put_if\nleaving put_if\nclosed 2\nafter",
    );
}

/// A method that keeps its receiver hands over the receiver it was given, so
/// a method whose receiver is in turn the caller's lends nothing it does not
/// own: a trait-declared method lends its receiver, and the call inside it
/// funds the keeper with a reference of its own, released inside the keeper;
/// the caller of the trait method still releases its own one after the call.
#[test]
fn a_lent_receiver_handed_to_a_keeper_is_funded_and_its_caller_releases_it() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Base?

trait Relaying
    fn relay() int

class Base implements Relaying
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn put_on(self) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_on")
        return 0
    fn relay() int
        let r = self.put_on()
        println("leaving relay")
        return r

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed derived {self.id}")

fn main()
    Base(1).relay()
    println("after base")
    Derived(2).relay()
    println("after derived")
"#,
        "on shelf\nleaving put_on\nleaving relay\nclosed 1\nafter base\non shelf\nleaving put_on\nleaving relay\nclosed derived 2\nafter derived",
    );
}

/// A method that forwards its receiver to a keeper keeps it too, so the
/// receiver travels by transfer from the outermost caller to the shelf, and
/// the hook runs inside the innermost method, as the derived class's own.
#[test]
fn a_receiver_forwarded_to_a_keeper_is_released_where_the_innermost_owner_ends() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn put_on(self) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_on")
        return 0
    fn forward() int
        println("forwarding")
        let r = self.put_on()
        println("leaving forward")
        return r

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed derived {self.id}")

fn main()
    let b = Base(1)
    b.forward()
    println("after base")
    Derived(2).forward()
    println("after derived")
"#,
        "forwarding\non shelf\nclosed 1\nleaving put_on\nleaving forward\nafter base\nforwarding\non shelf\nclosed derived 2\nleaving put_on\nleaving forward\nafter derived",
    );
}

/// A method the compiler calls for an operator is reached without a written
/// call, and every such call lends: the operator's receiver keeps the late
/// order, released by the caller after the expression.
#[test]
fn an_operator_method_keeping_its_receiver_still_lends_it() {
    assert_heap_guard_output(
        r#"
use system.ops

struct Shelf
    slot Handle?

class Handle implements Addable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    public fn concat(other Self) Self
        let sum = self.id + other.id
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving concat")
        return Handle(sum)

fn main()
    let a = Handle(1)
    let b = Handle(2)
    let c = a + b
    println(f"sum {c.id}")
"#,
        "on shelf\nleaving concat\nsum 3\nclosed 3\nclosed 2\nclosed 1",
    );
}

/// A method named `next` is one a `for` loop calls without a written call,
/// so it lends its receiver even where it is called by name.
#[test]
fn a_next_method_keeping_its_receiver_still_lends_it() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Ticker
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed ticker {self.id}")
    fn next() int?
        if true
            let _shelf = TickerShelf(slot: Some(self))
            println("ticked")
        println("leaving next")
        return None

struct TickerShelf
    slot Ticker?

fn main()
    let t = Ticker(1)
    t.next()
    println("after")
"#,
        ),
        "ticked\nleaving next\nclosed ticker 1\nafter",
    );
}

/// A name some trait declares takes a vtable slot in every class, so a
/// method of that name lends its receiver even in a class that implements no
/// trait at all.
#[test]
fn a_keeper_named_like_a_selector_elsewhere_still_lends_its_receiver() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Storing
    fn store(n int) int

class Unrelated implements Storing
    fn init()
        println("unrelated")
    fn store(n int) int
        return n

class Locker
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed locker {self.id}")
    fn store(n int) int
        if true
            let _shelf = LockerShelf(slot: Some(self))
            println("stored")
        println("leaving store")
        return n

struct LockerShelf
    slot Locker?

fn main()
    let l = Locker(1)
    l.store(2)
    println("after")
"#,
        ),
        "stored\nleaving store\nclosed locker 1\nafter",
    );
}

/// An `out` parameter is never owned, but it does not stop the method owning
/// the parameter it keeps: that one is released where the shelf ends, and the
/// `out` value is written back.
#[test]
fn a_keeper_with_an_out_parameter_still_owns_the_parameter_it_keeps() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Counter
    fn init()
        println("counter")
    fn shelve(h Handle, count out int) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("shelved")
        println("leaving shelve")
        count = count + 1
        return count

fn main()
    let c = Counter()
    var count = 0
    c.shelve(Handle(1), count)
    println(f"count {count}")
"#,
        ),
        "counter\nshelved\nclosed 1\nleaving shelve\ncount 1",
    );
}

/// A receiver read out of a list element is funded for the method with a
/// reference of its own, and the list keeps the element until it ends.
#[test]
fn an_element_receiver_is_funded_and_its_list_still_releases_it() {
    assert_heap_guard_output(
        &with_handle(
            r#"
use system.collections.list

fn main()
    let l = List([Handle(1)])
    l[0].put_on(2)
    println("after")
"#,
        ),
        "on shelf\nleaving put_on\nafter\nclosed 1",
    );
}

/// A keeper that returns its receiver hands it to the call's temporary,
/// which the next keeper in the chain is handed over in turn.
#[test]
fn a_receiver_chained_through_a_keeper_returning_it_is_handed_over_twice() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Passer
    fn init()
        println("passer")
    fn pass(h Handle) Handle
        println("passing")
        return h

fn main()
    Passer().pass(Handle(1)).put_on(2)
    println("after")
"#,
        ),
        "passer\npassing\non shelf\nclosed 1\nleaving put_on\nafter",
    );
}

/// A method that keeps its receiver on some path consumes it on every path,
/// so any later use, through any method, is refused.
#[test]
fn a_receiver_used_through_another_method_after_a_partial_keep_is_refused() {
    assert_compiler_error(
        &with_gated_handle(
            r#"
fn main()
    let h = Handle(1)
    h.put_if(0)
    println(f"{h.peek()}")
"#,
        ),
        "'h' was consumed by 'Handle.put_if' and cannot be used again",
    );
}

/// A derived instance calling an inherited keeper is handed over even when
/// its class overrides some other method: only the overridden name takes a
/// vtable slot.
#[test]
fn an_inherited_keeper_on_a_class_overriding_another_method_owns_its_receiver() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed base {self.id}")
    fn describe() String
        return "base"
    fn put_on(self) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving")
        return 0

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed derived {self.id}")
    fn describe() String
        return "derived"

fn main()
    let d = Derived(3)
    println(d.describe())
    d.put_on()
    println("after")
"#,
        "derived\non shelf\nclosed derived 3\nleaving\nafter",
    );
}

/// An argument read from the receiver is read before the receiver is handed
/// over, so a method that releases its receiver before reading the argument
/// still reads the value the caller passed.
#[test]
fn an_argument_read_from_a_kept_receiver_is_read_before_the_hand_over() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Tag?

class Tag
    public var label String
    fn init(label String)
        self.label = label
    fn drop(self)
        println(f"closed {self.label}")
    fn put(label String) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println(f"label {label}")
        return 0

fn main()
    let t = Tag("one")
    t.put(t.label)
    println("after")
"#,
        "on shelf\nclosed one\nlabel one\nafter",
    );
}

/// Reading a keeper without calling it is refused like reading any method
/// as a value.
#[test]
fn a_keeper_read_as_a_value_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let f = h.put_on
    println("after")
"#,
        ),
        "MER_TYP_078",
    );
}

/// A keeper that writes its receiver before keeping it, called on a field:
/// the method is funded with a reference of its own, so the field the caller
/// reads afterwards is the one the caller holds.
#[test]
fn a_writing_keeper_called_on_a_field_leaves_the_field_readable() {
    assert_heap_guard_output(
        r#"
struct Shelf
    slot Counter?

class Counter
    public var x int
    fn init(x int)
        self.x = x
    fn drop(self)
        println(f"closed {self.x}")
    fn bump_and_keep() int
        self.x = self.x + 1
        let bumped = self.x
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving")
        return bumped

struct Holder
    c Counter

fn main()
    let h = Holder(c: Counter(1))
    h.c.bump_and_keep()
    println(f"field {h.c.x}")
"#,
        "on shelf\nleaving\nfield 2\nclosed 2",
    );
}
