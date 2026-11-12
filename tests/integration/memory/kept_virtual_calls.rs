// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A call through a trait or an overridable method follows the convention of
//! the vtable slot it reads. A slot is numbered within one hierarchy, and it
//! owns every resource position some body filling it keeps; a body keeping
//! less is reached through an adapter that releases, before control returns
//! to the caller, what it was handed and the body did not keep. Every test
//! reads the exact order of the hooks' output under the heap guard.

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
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

const TAKERS: &str = r#"
trait Taker
    fn take(h Handle) int

class Keeper implements Taker
    fn init()
        println("keeper")
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("kept")
        println("leaving keeper take")
        return 1

class Reader implements Taker
    fn init()
        println("reader")
    fn take(h Handle) int
        println(f"read {h.id}")
        return 2
"#;

fn with_takers(program: &str) -> String {
    with_handle(&format!("{TAKERS}\n{program}"))
}

fn with_list(program: &str) -> String {
    format!("use system.collections.list\n{}", with_takers(program))
}

/// A trait method one implementor keeps the parameter of owns it: the keeper
/// releases it where its shelf ends, and the reader's adapter releases it as
/// the reader returns, before the caller prints anything.
#[test]
fn a_trait_call_hands_the_parameter_its_slot_owns_to_every_implementor() {
    assert_heap_guard_output(
        &with_takers(
            r#"
fn run(t Taker, h Handle)
    t.take(h)
    println("after take")

fn main()
    run(Keeper(), Handle(1))
    run(Reader(), Handle(2))
    println("end")
"#,
        ),
        "keeper\nkept\nclosed 1\nleaving keeper take\nafter take\nreader\nread 2\nclosed 2\nafter take\nend",
    );
}

const ABSTRACT_TAKERS: &str = r#"
abstract class Base
    abstract fn take(h Handle) int

class Keeper extends Base
    fn init()
        println("keeper")
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("kept")
        println("leaving keeper take")
        return 1

class Reader extends Base
    fn init()
        println("reader")
    fn take(h Handle) int
        println(f"read {h.id}")
        return 2
"#;

/// An abstract method's slot owns what its keeping override keeps, and a
/// static call to that override follows the override's own convention: both
/// release the handle where the shelf ends.
#[test]
fn an_abstract_call_and_a_static_call_release_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{ABSTRACT_TAKERS}\n{}",
            r#"
fn main()
    let k Base = Keeper()
    k.take(Handle(1))
    println("after virtual keeper")
    let k2 = Keeper()
    k2.take(Handle(3))
    println("after static keeper")
    let r Base = Reader()
    r.take(Handle(2))
    println("after virtual reader")
"#
        )),
        "keeper\nkept\nclosed 1\nleaving keeper take\nafter virtual keeper\nkeeper\nkept\nclosed 3\nleaving keeper take\nafter static keeper\nreader\nread 2\nclosed 2\nafter virtual reader",
    );
}

const SHELVERS: &str = r#"
trait Shelving
    fn shelve(n int) int

struct KeeperShelf
    slot Keeper?

class Keeper implements Shelving
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed keeper {self.id}")
    fn shelve(n int) int
        if true
            let _shelf = KeeperShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n

class Plain implements Shelving
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed plain {self.id}")
    fn shelve(n int) int
        println(f"plain {self.id}")
        return n

fn make(keeps bool, id int) Shelving
    if keeps
        return Keeper(id)
    return Plain(id)
"#;

/// A slot owning the receiver hands a temporary receiver over: the keeper
/// releases it where its shelf ends, and the non-keeping implementor's
/// adapter releases it as that implementor returns.
#[test]
fn a_slot_owning_the_receiver_releases_it_inside_a_non_keepers_adapter() {
    assert_heap_guard_output(
        &format!(
            "{SHELVERS}\n{}",
            r#"
fn main()
    make(true, 1).shelve(5)
    println("after keeper")
    make(false, 2).shelve(6)
    println("after plain")
"#
        ),
        "shelved\nclosed keeper 1\nleaving shelve\nafter keeper\nplain 2\nclosed plain 2\nafter plain",
    );
}

/// A non-keeping implementor of a slot that owns a parameter releases it in
/// its adapter even when the caller's binding was its last holder.
#[test]
fn a_non_keepers_adapter_releases_the_parameter_its_slot_owns() {
    assert_heap_guard_output(
        &with_takers(
            r#"
fn main()
    let t Taker = Reader()
    let h = Handle(7)
    t.take(h)
    println("after")
"#,
        ),
        "reader\nread 7\nclosed 7\nafter",
    );
}

/// A method of an unrelated class that shares a trait method's name is no
/// slot of that trait: a reader of that name lends its parameter, which stays
/// the caller's, and a keeper of that name owns what it keeps.
#[test]
fn an_unrelated_method_named_like_a_trait_method_keeps_its_own_convention() {
    assert_heap_guard_output(
        &with_takers(
            r#"
class Archive
    fn init()
        println("archive")
    fn take(h Handle) int
        println(f"archived {h.id}")
        return 0

class Vault
    fn init()
        println("vault")
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("vaulted")
        println("leaving vault take")
        return 0

fn main()
    let a = Archive()
    let h = Handle(4)
    a.take(h)
    println(f"still {h.id}")
    let v = Vault()
    v.take(Handle(5))
    println("after vault")
    let t Taker = Keeper()
    t.take(Handle(6))
    println("end")
"#,
        ),
        "archive\narchived 4\nstill 4\nvault\nvaulted\nclosed 5\nleaving vault take\nafter vault\nkeeper\nkept\nclosed 6\nleaving keeper take\nend\nclosed 4",
    );
}

/// A class implementing two traits that both declare a method fills one slot
/// of each, and each slot keeps its own convention: the one a keeper shares
/// owns the parameter, the other lends it.
#[test]
fn two_traits_declaring_one_method_fill_two_slots_with_their_own_conventions() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Front
    fn take(h Handle) int

trait Back
    fn take(h Handle) int

class FrontKeeper implements Front
    fn init()
        println("front keeper")
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("kept")
        println("leaving keeper take")
        return 1

class Both implements Front, Back
    fn init()
        println("both")
    fn take(h Handle) int
        println(f"both read {h.id}")
        return 0

fn through_back(b Back, h Handle)
    b.take(h)
    println(f"back left {h.id}")

fn through_front(f Front, h Handle)
    f.take(h)
    println("front done")

fn main()
    through_back(Both(), Handle(1))
    through_front(Both(), Handle(2))
    through_front(FrontKeeper(), Handle(3))
"#,
        ),
        "both\nboth read 1\nback left 1\nclosed 1\nboth\nboth read 2\nclosed 2\nfront done\nfront keeper\nkept\nclosed 3\nleaving keeper take\nfront done",
    );
}

/// Through the slot that lends, the caller's binding stays usable after the
/// call, though another trait's slot of the same name owns it.
#[test]
fn a_binding_passed_through_a_lending_slot_stays_usable() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Front
    fn take(h Handle) int

trait Back
    fn take(h Handle) int

class FrontKeeper implements Front
    fn init()
        println("front keeper")
    fn take(h Handle) int
        let _shelf = Shelf(slot: Some(h))
        return 1

class Both implements Front, Back
    fn init()
        println("both")
    fn take(h Handle) int
        println(f"both read {h.id}")
        return 0

fn main()
    let b Back = Both()
    let h = Handle(8)
    b.take(h)
    b.take(h)
    println(f"kept {h.id}")
"#,
        ),
        "both\nboth read 8\nboth read 8\nkept 8\nclosed 8",
    );
}

/// A generic class fills its slot once per instantiation, each through an
/// adapter of its own that releases what the slot handed it.
#[test]
fn a_generic_implementor_is_adapted_at_each_instantiation() {
    assert_heap_guard_output(
        &with_takers(
            r#"
class Labelled<T> implements Taker
    public var label T
    fn init(label T)
        self.label = label
    fn take(h Handle) int
        println(f"labelled read {h.id}")
        return 0

fn run(t Taker, h Handle)
    t.take(h)
    println("after take")

fn main()
    run(Keeper(), Handle(1))
    run(Labelled<int>(7), Handle(2))
    run(Labelled<String>("x"), Handle(3))
"#,
        ),
        "keeper\nkept\nclosed 1\nleaving keeper take\nafter take\nlabelled read 2\nclosed 2\nafter take\nlabelled read 3\nclosed 3\nafter take",
    );
}

/// A binding a virtual call's slot owns is consumed by the call.
#[test]
fn a_binding_a_virtual_calls_slot_owns_cannot_be_used_after_the_call() {
    assert_compiler_error(
        &with_takers(
            r#"
fn run(t Taker, h Handle)
    t.take(h)
    println(f"{h.id}")

fn main()
    run(Reader(), Handle(1))
"#,
        ),
        "MER_OWN_003",
    );
}

/// A binding passed to a slot no implementor keeps stays the caller's.
#[test]
fn a_binding_a_virtual_calls_slot_lends_stays_usable_after_the_call() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Peeking
    fn peek(h Handle) int

class Peeker implements Peeking
    fn init()
        println("peeker")
    fn peek(h Handle) int
        return h.id

fn main()
    let p Peeking = Peeker()
    let h = Handle(9)
    p.peek(h)
    println(f"peeked {p.peek(h)}")
"#,
        ),
        "peeker\npeeked 9\nclosed 9",
    );
}

const HOLDER: &str = r#"
class Holder
    var h Handle
    fn init(h Handle)
        self.h = h
    fn go(t Taker)
        t.take(self.h)
        println("went")
"#;

/// A field handed to a slot that owns it is a copy the call funds: each
/// non-keeper's adapter releases its own reference, and the holder still
/// holds the field, released once where the holder ends.
#[test]
fn a_field_handed_to_an_owning_slot_is_funded_and_its_holder_releases_it() {
    assert_heap_guard_output(
        &with_takers(&format!(
            "{HOLDER}\n{}",
            r#"
fn main()
    let k = Holder(Handle(1))
    k.go(Reader())
    println("mid")
    k.go(Reader())
    println("end")
"#
        )),
        "reader\nread 1\nwent\nmid\nreader\nread 1\nwent\nend\nclosed 1",
    );
}

/// A field handed to a keeper through its slot is funded too: the keeper's
/// shelf releases its own reference, and the field stays readable.
#[test]
fn a_field_handed_to_a_keeper_through_its_slot_stays_its_holders() {
    assert_heap_guard_output(
        &with_takers(&format!(
            "{HOLDER}\n{}",
            r#"
fn main()
    let k = Holder(Handle(1))
    let t Taker = Keeper()
    t.take(k.h)
    println("after")
    println(f"{k.h.id}")
"#
        )),
        "keeper\nkept\nleaving keeper take\nafter\n1\nclosed 1",
    );
}

/// An element handed to a slot that owns it is funded, through a binding of
/// the caller's and through a lent parameter alike, so the list still holds
/// it after each call.
#[test]
fn an_element_and_a_lent_parameter_handed_to_an_owning_slot_are_funded() {
    assert_heap_guard_output(
        &with_list(
            r#"
fn use_it(t Taker, h Handle)
    t.take(h)
    println("used")

fn main()
    var hs List<Handle> = List<Handle>()
    hs.push(Handle(1))
    let t Taker = Reader()
    t.take(hs[0])
    println("after elem")
    println(f"{hs[0].id}")
    use_it(Reader(), hs[0])
    use_it(Reader(), hs[0])
    println("end")
"#,
        ),
        "reader\nread 1\nafter elem\n1\nreader\nread 1\nused\nreader\nread 1\nused\nend\nclosed 1",
    );
}

/// A trait-typed binding whose implementors may run a drop hook is a
/// resource, so a slot owning the receiver takes the binding over: the
/// keeper's shelf is its new owner, and the hook runs where the shelf ends.
#[test]
fn a_trait_typed_resource_binding_an_owning_slot_takes_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &format!(
            "{SHELVERS}\n{}",
            r#"
fn main()
    let s Shelving = Keeper(1)
    println(f"{s.shelve(1)}")
    println("end")
"#
        ),
        "shelved\nclosed keeper 1\nleaving shelve\n1\nend",
    );
}

/// A trait-typed resource binding a slot owning the receiver took over
/// cannot be called through again: the second call is reported.
#[test]
fn a_trait_typed_resource_binding_handed_twice_to_an_owning_slot_is_reported() {
    let program = format!(
        "{SHELVERS}\n{}",
        r#"
fn main()
    let s Shelving = Keeper(1)
    println(f"{s.shelve(1)}")
    println(f"{s.shelve(2)}")
"#
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":40:16");
}

/// A trait-typed resource parameter a slot owning the receiver took cannot be
/// read after the call, as a binding of the function's own cannot.
#[test]
fn a_trait_typed_resource_parameter_an_owning_slot_took_cannot_be_used_after() {
    assert_compiler_error(
        &format!(
            "{SHELVERS}\n{}",
            r#"
fn run(s Shelving)
    println(f"{s.shelve(1)}")
    println(f"{s.shelve(2)}")

fn main()
    run(Keeper(1))
"#
        ),
        "MER_OWN_003",
    );
}

/// A trait-typed binding whose implementors run no drop hook is no resource:
/// a slot keeping the receiver lends it, and the binding stays usable.
#[test]
fn a_trait_typed_binding_over_hook_free_implementors_stays_usable() {
    assert_heap_guard_output(
        r#"
trait Noting
    fn note(n int) int

struct NoteShelf
    slot Note?

class Note implements Noting
    public var id int
    fn init(id int)
        self.id = id
    fn note(n int) int
        let _shelf = NoteShelf(slot: Some(self))
        return self.id + n

fn main()
    let s Noting = Note(1)
    println(f"{s.note(1)}")
    println(f"{s.note(2)}")
"#,
        "2\n3",
    );
}

/// A trait-typed resource parameter handed to a slot owning the receiver is
/// kept by the function, so the function owns it and hands it on: the
/// keeper releases it where its shelf ends, and a non-keeper's adapter as it
/// returns, each before the call that handed it over returns.
#[test]
fn a_trait_typed_resource_parameter_handed_to_an_owning_slot_is_the_functions() {
    assert_heap_guard_output(
        &format!(
            "{SHELVERS}\n{}",
            r#"
fn run(s Shelving)
    println(f"{s.shelve(1)}")
    println("after")

fn main()
    run(Keeper(1))
    run(Plain(2))
    println("end")
"#
        ),
        "shelved\nclosed keeper 1\nleaving shelve\n1\nafter\nplain 2\nclosed plain 2\n1\nafter\nend",
    );
}

const DEFAULT_TAKERS: &str = r#"
trait Taker
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("kept by default")
        println("leaving default take")
        return 1

class Inheritor implements Taker
    fn init()
        println("inheritor")

class Reader implements Taker
    fn init()
        println("reader")
    fn take(h Handle) int
        println(f"read {h.id}")
        return 2
"#;

/// A trait default that keeps its parameter fills the slot of every class
/// inheriting it, so the slot owns the parameter: the default lends and
/// retains what it keeps, its adapter releases the slot's reference, and an
/// overriding reader's adapter releases it as the reader returns. A static
/// call to the inherited default keeps the default's own convention.
#[test]
fn a_keeping_trait_default_makes_its_slot_own_the_parameter() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{DEFAULT_TAKERS}\n{}",
            r#"
fn run(t Taker, h Handle)
    t.take(h)
    println("after take")

fn main()
    run(Inheritor(), Handle(1))
    run(Reader(), Handle(2))
    let i = Inheritor()
    i.take(Handle(3))
    println("after static")
    let t Taker = Reader()
    let h = Handle(4)
    t.take(h)
    println("end")
"#
        )),
        "inheritor\nkept by default\nleaving default take\nclosed 1\nafter take\nreader\nread 2\nclosed 2\nafter take\ninheritor\nkept by default\nleaving default take\nclosed 3\nafter static\nreader\nread 4\nclosed 4\nend",
    );
}

/// A binding handed through a slot a keeping default fills is consumed.
#[test]
fn a_binding_handed_to_a_slot_a_keeping_default_fills_cannot_be_used_after() {
    assert_compiler_error(
        &with_handle(&format!(
            "{DEFAULT_TAKERS}\n{}",
            r#"
fn main()
    let t Taker = Inheritor()
    let h = Handle(4)
    t.take(h)
    println(f"{h.id}")
"#
        )),
        "MER_OWN_003",
    );
}

/// A generic class's method fills its slot at every instantiation; one that
/// keeps makes the slot own the parameter, and every instantiation, reading
/// or keeping, is reached through an adapter.
#[test]
fn a_keeping_generic_class_method_makes_its_slot_own_the_parameter() {
    assert_heap_guard_output(
        &with_takers(
            r#"
class Boxed<T> implements Taker
    var v T
    fn init(v T)
        self.v = v
    fn take(h Handle) int
        println(f"boxed read {h.id}")
        return 2

class GKeeper<T> implements Taker
    var v T
    fn init(v T)
        self.v = v
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("gkept")
        println("leaving gkeeper")
        return 3

fn run(t Taker, h Handle)
    println(f"{t.take(h)}")

fn main()
    run(Boxed<int>(1), Handle(2))
    run(Boxed<String>("s"), Handle(3))
    run(GKeeper<int>(1), Handle(4))
    println("end")
"#,
        ),
        "boxed read 2\nclosed 2\n2\nboxed read 3\nclosed 3\n2\ngkept\nleaving gkeeper\nclosed 4\n3\nend",
    );
}

/// A binding a slot owns cannot be handed over again on the next iteration
/// of a loop.
#[test]
fn a_binding_an_owning_slot_takes_in_a_loop_is_reported_at_the_call() {
    let program = with_takers(
        r#"
fn run(t Taker, h Handle)
    for i in 0..2
        t.take(h)

fn main()
    run(Reader(), Handle(1))
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":36:16");
}

/// The same binding handed twice to an owning slot is reported at the
/// second call.
#[test]
fn a_binding_handed_twice_to_an_owning_slot_is_reported_at_the_second_call() {
    let program = with_takers(
        r#"
fn run(t Taker, h Handle)
    t.take(h)
    t.take(h)

fn main()
    run(Reader(), Handle(1))
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":36:12");
}

/// A `super` call follows the parent body's own convention: the child,
/// which hands its parameter to a keeping parent, keeps it too, and both
/// calls through the base release it where the parent's shelf ends.
#[test]
fn a_super_call_to_a_keeping_parent_follows_the_parents_convention() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Base
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("base kept")
        println("leaving base")
        return 1

class Child extends Base
    fn take(h Handle) int
        println(f"child read {h.id}")
        return super.take(h)

fn main()
    let b Base = Child()
    b.take(Handle(1))
    println("after child")
    let p Base = Base()
    p.take(Handle(2))
    println("end")
"#,
        ),
        "child read 1\nbase kept\nclosed 1\nleaving base\nafter child\nbase kept\nclosed 2\nleaving base\nend",
    );
}

/// In a chain where the top and the bottom keep and the middle reads, the
/// slot owns the parameter, and each body called through the base releases
/// it where it ends: the keepers at their shelves, the reader's adapter as
/// it returns.
#[test]
fn a_three_deep_chain_with_a_reader_between_keepers_shares_one_convention() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class A
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("a kept")
        println("leaving a")
        return 1

class B extends A
    fn take(h Handle) int
        println(f"b read {h.id}")
        return 2

class C extends B
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("c kept")
        println("leaving c")
        return 3

fn run(a A, h Handle)
    a.take(h)
    println("after")

fn main()
    run(A(), Handle(1))
    run(B(), Handle(2))
    run(C(), Handle(3))
"#,
        ),
        "a kept\nclosed 1\nleaving a\nafter\nb read 2\nclosed 2\nafter\nc kept\nclosed 3\nleaving c\nafter",
    );
}

/// A slot can own both the receiver and a parameter: the keeper releases
/// both where its shelf ends, and the reader's adapter releases both as the
/// reader returns, in reverse declaration order.
#[test]
fn a_slot_owning_the_receiver_and_a_parameter_releases_both_where_the_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Pairing
    fn pair(h Handle) int

struct PairShelf
    owner Pairer?
    handle Handle?

class Pairer implements Pairing
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed pairer {self.id}")
    fn pair(h Handle) int
        if true
            let _shelf = PairShelf(owner: Some(self), handle: Some(h))
            println("paired")
        println("leaving pair")
        return 1

class Peeker implements Pairing
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed peeker {self.id}")
    fn pair(h Handle) int
        println(f"peeked {h.id}")
        return 2

fn make(keeps bool, id int) Pairing
    if keeps
        return Pairer(id)
    return Peeker(id)

fn main()
    make(true, 1).pair(Handle(10))
    println("after pairer")
    make(false, 2).pair(Handle(20))
    println("end")
"#,
        ),
        "paired\nclosed pairer 1\nclosed 10\nleaving pair\nafter pairer\npeeked 20\nclosed 20\nclosed peeker 2\nend",
    );
}

/// A slot owning a parameter and lending the receiver leaves a `let`
/// receiver the caller's, usable for the next call.
#[test]
fn a_receiver_a_slot_lends_stays_usable_while_the_parameter_is_handed_over() {
    assert_heap_guard_output(
        &with_takers(
            r#"
fn main()
    let t Taker = Keeper()
    t.take(Handle(1))
    t.take(Handle(2))
    println("end")
"#,
        ),
        "keeper\nkept\nclosed 1\nleaving keeper take\nkept\nclosed 2\nleaving keeper take\nend",
    );
}

/// Each element of a list of trait values answers through the slot, the
/// keeper and the reader alike.
#[test]
fn a_loop_over_trait_values_hands_each_a_fresh_resource_through_the_slot() {
    assert_heap_guard_output(
        &with_list(
            r#"
fn main()
    var ts List<Taker> = List<Taker>()
    ts.push(Keeper())
    ts.push(Reader())
    var i = 0
    for t in ts
        t.take(Handle(i))
        i = i + 1
    println("end")
"#,
        ),
        "keeper\nreader\nkept\nclosed 0\nleaving keeper take\nread 1\nclosed 1\nend",
    );
}

/// A generic implementor that keeps on one path only lends what it keeps
/// and is reached through an adapter: on the keeping path its retain holds
/// the value until its shelf ends and the adapter releases the slot's
/// reference after it returns; on the other the adapter's release is the
/// last.
#[test]
fn a_conditional_keep_in_an_adapted_implementor_releases_on_both_paths() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Gated
    fn take(h Handle, keep bool) int

class Gate<T> implements Gated
    var v T
    fn init(v T)
        self.v = v
    fn take(h Handle, keep bool) int
        if keep
            let _shelf = Shelf(slot: Some(h))
            println("gate kept")
        println("leaving gate")
        return 1

fn run(g Gated, h Handle, keep bool)
    g.take(h, keep)
    println("after")

fn main()
    run(Gate<int>(0), Handle(1), true)
    run(Gate<int>(0), Handle(2), false)
"#,
        ),
        "gate kept\nleaving gate\nclosed 1\nafter\nleaving gate\nclosed 2\nafter",
    );
}

/// A generic implementor instantiated at a resource type is adapted beside
/// its scalar instantiations, and its own resource is released with it.
#[test]
fn a_generic_implementor_at_a_resource_type_is_adapted_beside_scalar_ones() {
    assert_heap_guard_output(
        &with_takers(
            r#"
class Labelled<T> implements Taker
    public var label T
    fn init(label T)
        self.label = label
    fn take(h Handle) int
        println(f"labelled read {h.id}")
        return 0

fn run(t Taker, h Handle)
    t.take(h)
    println("after take")

fn main()
    run(Labelled<int>(7), Handle(1))
    run(Labelled<Handle>(Handle(9)), Handle(2))
    println("end")
"#,
        ),
        "labelled read 1\nclosed 1\nafter take\nlabelled read 2\nclosed 2\nafter take\nclosed 9\nend",
    );
}

/// An implementor declared in another module fills the slot as one in the
/// program's own file does.
#[test]
fn an_implementor_from_another_module_follows_the_slots_convention() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.takers.{Handle, Taker, Reader}\n",
                    "\n",
                    "struct Shelf\n",
                    "    slot Handle?\n",
                    "\n",
                    "class Keeper implements Taker\n",
                    "    fn take(h Handle) int\n",
                    "        if true\n",
                    "            let _shelf = Shelf(slot: Some(h))\n",
                    "            println(\"kept\")\n",
                    "        println(\"leaving keeper\")\n",
                    "        return 1\n",
                    "\n",
                    "fn run(t Taker, h Handle)\n",
                    "    t.take(h)\n",
                    "    println(\"after\")\n",
                    "\n",
                    "fn main()\n",
                    "    run(Keeper(), Handle(1))\n",
                    "    run(Reader(), Handle(2))\n",
                ),
            ),
            (
                "takers.mi",
                concat!(
                    "use system.io\n",
                    "\n",
                    "public class Handle\n",
                    "    public var id int\n",
                    "    fn init(id int)\n",
                    "        self.id = id\n",
                    "    fn drop(self)\n",
                    "        println(f\"closed {self.id}\")\n",
                    "\n",
                    "public trait Taker\n",
                    "    fn take(h Handle) int\n",
                    "\n",
                    "public class Reader implements Taker\n",
                    "    fn take(h Handle) int\n",
                    "        println(f\"read {h.id}\")\n",
                    "        return 2\n",
                ),
            ),
        ],
        "kept\nclosed 1\nleaving keeper\nafter\nread 2\nclosed 2\nafter",
    );
}

/// A `for` binding is the element it iterates and owns no reference of its
/// own, so a slot owning the receiver is handed a funded copy of it: the
/// keeper's shelf releases its copy, the non-keeper's adapter releases its
/// copy, and every hook runs once, where the list ends.
#[test]
fn a_for_binding_handed_to_a_slot_owning_the_receiver_is_a_funded_copy() {
    assert_heap_guard_output(
        &format!(
            "use system.collections.list\n{SHELVERS}\n{}",
            r#"
fn main()
    var l = List<Shelving>()
    l.push(Keeper(3))
    l.push(Plain(4))
    for s in l
        println(f"{s.shelve(1)}")
    println("end")
"#
        ),
        "shelved\nleaving shelve\n1\nplain 4\n1\nend\nclosed keeper 3\nclosed plain 4",
    );
}

/// A pattern's payload binding reads the payload and owns no reference of
/// its own, so a slot owning the receiver is handed a funded copy of it, and
/// the hook runs once, where the optional ends.
#[test]
fn a_payload_binding_handed_to_a_slot_owning_the_receiver_is_a_funded_copy() {
    assert_heap_guard_output(
        &format!(
            "{SHELVERS}\n{}",
            r#"
fn main()
    let o Shelving? = Some(Keeper(5))
    match o
        Some(s): println(f"{s.shelve(1)}")
        None: println("none")
    println("end")
"#
        ),
        "shelved\nleaving shelve\n1\nend\nclosed keeper 5",
    );
}

/// A `for` binding reaching a hook-free implementor through the adapter of a
/// slot owning the receiver is handed over as a funded copy: the adapter
/// releases the copy, and the list still holds the element.
#[test]
fn a_for_binding_handed_through_a_non_keepers_adapter_is_a_funded_copy() {
    assert_heap_guard_output(
        r#"
use system.collections.list

trait Lighting
    fn light(n int) int

struct LampShelf
    slot Torch?

class Torch implements Lighting
    fn drop(self)
        println("closed torch")
    fn light(n int) int
        let _shelf = LampShelf(slot: Some(self))
        return n

class Lamp implements Lighting
    public var id int
    fn init(id int)
        self.id = id
    fn light(n int) int
        println(f"lamp {self.id}")
        return n

fn main()
    var l = List<Lighting>()
    l.push(Lamp(4))
    for s in l
        println(f"{s.light(1)}")
    println(f"{l.length()}")
    println("end")
"#,
        "lamp 4\n1\n1\nend",
    );
}
