// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A resource read out of a place is borrowed: it may be lent, but a position
//! that keeps what it is handed may not take it. Handing one to a keeper — a
//! function keeping it, a method keeping its receiver, a call through a
//! function value, a trait slot owning it — is refused with MER_OWN_008, at
//! the line handing it over, with the repair that compiles for that place:
//! `replace` for an `out` parameter or a receiver the method restores,
//! `take()` for an optional, removing the element for a list element or a
//! loop binding. Each repair runs, every hook once, in the order asserted.
//!
//! A `match` on a value the scope owns hands its payload over instead: the
//! keeper consumes the value matched, which ends at the keep. A plain
//! resource field is still handed over as a copy its holder keeps funding.

use super::super::utils::*;

pub(super) const CODE: &str = "MER_OWN_008";

const PRELUDE: &str = r#"use system.io
use system.memory
use system.collections.list

struct Shelf
    slot Res?

class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn shelve() int
        if true
            let _s = Shelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return 0

class Holder
    public var slot Res?
    public var conn Res
    fn init(id int)
        self.slot = Some(Res(id))
        self.conn = Res(id + 10)

trait Taker
    fn hold(r Res) int

class Keeper implements Taker
    fn init()
        return
    fn hold(r Res) int
        if true
            let _s = Shelf(slot: Some(r))
            println("held")
        println("leaving hold")
        return 0

fn keep(r Res) int
    if true
        let _s = Shelf(slot: Some(r))
        println("kept")
    println("leaving keep")
    return 0

fn keep_opt(r Res?) int
    let _s = Shelf(slot: r)
    println("kept opt")
    return 0
"#;

/// The program `PRELUDE` opens, followed by `rest`.
pub(super) fn program(rest: &str) -> String {
    format!("{PRELUDE}{rest}")
}

/// Asserts `rest` is refused with MER_OWN_008 at the line containing
/// `handing`, the message naming `place` and the help carrying `repair`, the
/// whole spelling it suggests.
pub(super) fn assert_refused(rest: &str, handing: &str, place: &str, repair: &str) {
    let diagnostic = assert_refused_at(&program(rest), CODE, handing);
    assert!(
        diagnostic.message.contains(place),
        "the refusal names '{place}': {diagnostic:#?}"
    );
    let help = diagnostic.help.as_deref().unwrap_or_default();
    assert!(
        help.contains(repair),
        "the refusal carries the repair '{repair}': {diagnostic:#?}"
    );
}

#[test]
fn an_optional_field_handed_to_a_keeping_function_is_refused() {
    assert_refused(
        r#"
fn main()
    let h = Holder(1)
    keep_opt(h.slot)
"#,
        "keep_opt(h.slot)",
        "h.slot",
        "`h.slot.take()`",
    );
}

#[test]
fn an_optional_element_handed_to_a_keeping_function_is_refused() {
    assert_refused(
        r#"
fn main()
    var slots = List<Res?>()
    slots.push(Some(Res(1)))
    keep_opt(slots[0])
"#,
        "keep_opt(slots[0])",
        "slots[0]",
        "`slots[0].take()`",
    );
}

#[test]
fn a_list_element_handed_to_a_keeping_function_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    keep(l[0])
"#,
        "keep(l[0])",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_list_element_used_as_a_kept_receiver_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    l[0].shelve()
"#,
        "l[0].shelve()",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_list_element_handed_through_a_function_value_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    let f = keep
    f(l[0])
"#,
        "f(l[0])",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_list_element_handed_to_an_owning_trait_slot_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    let t Taker = Keeper()
    t.hold(l[0])
"#,
        "t.hold(l[0])",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_payload_bound_from_an_optional_field_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn main()
    let h = Holder(1)
    match h.slot
        Some(x): keep(x)
        None: println("none")
"#,
        "Some(x): keep(x)",
        "h.slot",
        "`match h.slot.take()`",
    );
}

#[test]
fn a_payload_bound_from_an_optional_field_used_as_a_kept_receiver_is_refused() {
    assert_refused(
        r#"
fn main()
    let h = Holder(1)
    match h.slot
        Some(x): x.shelve()
        None: println("none")
"#,
        "Some(x): x.shelve()",
        "h.slot",
        "`match h.slot.take()`",
    );
}

#[test]
fn a_loop_binding_handed_to_a_keeping_function_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    for r in l
        keep(r)
"#,
        "        keep(r)",
        "'r'",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_loop_binding_used_as_a_kept_receiver_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    for r in l
        r.shelve()
"#,
        "r.shelve()",
        "'r'",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_loop_binding_handed_to_an_owning_trait_slot_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    let t Taker = Keeper()
    for r in l
        t.hold(r)
"#,
        "t.hold(r)",
        "'r'",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn a_local_bound_to_a_list_element_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    let mine = l[0]
    keep(mine)
"#,
        "keep(mine)",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn an_out_parameter_handed_to_a_keeping_function_is_refused() {
    assert_refused(
        r#"
fn swap(c out Res)
    keep(c)

fn main()
    var c = Res(1)
    swap(c)
"#,
        "    keep(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn an_out_parameter_used_as_a_kept_receiver_is_refused() {
    assert_refused(
        r#"
fn swap(c out Res)
    c.shelve()

fn main()
    var c = Res(1)
    swap(c)
"#,
        "c.shelve()",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn an_out_parameter_handed_through_a_function_value_is_refused() {
    assert_refused(
        r#"
fn swap(c out Res)
    let f = keep
    f(c)

fn main()
    var c = Res(1)
    swap(c)
"#,
        "f(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn an_out_parameter_handed_to_an_owning_trait_slot_is_refused() {
    assert_refused(
        r#"
fn give(t Taker, c out Res)
    t.hold(c)

fn main()
    var c = Res(1)
    give(Keeper(), c)
"#,
        "t.hold(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn a_local_bound_to_an_out_parameter_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn swap(c out Res)
    let old = c
    keep(old)

fn main()
    var c = Res(1)
    swap(c)
"#,
        "keep(old)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn an_optional_out_parameter_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn swap(c out Res?)
    keep_opt(c)

fn main()
    var c Res? = Some(Res(1))
    swap(c)
"#,
        "keep_opt(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn a_payload_bound_from_an_out_parameter_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn swap(c out Res?)
    match c
        Some(x): keep(x)
        None: println("none")

fn main()
    var c Res? = Some(Res(1))
    swap(c)
"#,
        "Some(x): keep(x)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

const SLOT: &str = r#"
enum Slot
    Full(Res)
    Empty
"#;

#[test]
fn a_restored_receiver_returned_through_a_local_is_refused() {
    assert_refused(
        &format!(
            "{SLOT}{}",
            r#"
    fn take_out() Slot
        let old = self
        self = Slot.Empty
        return old

fn main()
    var s = Slot.Full(Res(1))
    let t = s.take_out()
    match t
        Slot.Full(r): println(f"{r.id}")
        Slot.Empty: println("empty")
"#
        ),
        "return old",
        "'self'",
        "`return replace(self, <new value>)` (use system.memory)",
    );
}

#[test]
fn a_restored_receiver_returned_early_is_refused() {
    assert_refused(
        &format!(
            "{SLOT}{}",
            r#"
    fn take_out(early bool) Slot
        let old = self
        if early
            return old
        self = Slot.Empty
        return Slot.Empty

fn main()
    var s = Slot.Full(Res(1))
    let t = s.take_out(true)
    match t
        Slot.Full(r): println(f"{r.id}")
        Slot.Empty: println("empty")
"#
        ),
        "            return old",
        "'self'",
        "`return replace(self, <new value>)` (use system.memory)",
    );
}

#[test]
fn replacing_an_out_parameter_hands_its_old_value_to_the_keeper() {
    assert_heap_guard_output(
        &program(
            r#"
fn swap(c out Res)
    keep(replace(c, Res(9)))

fn main()
    var c = Res(1)
    swap(c)
    println(f"now {c.id}")
"#,
        ),
        "kept\nclosed 1\nleaving keep\nnow 9\nclosed 9",
    );
}

#[test]
fn replacing_a_restored_receiver_hands_its_old_value_back() {
    assert_heap_guard_output(
        &program(&format!(
            "{SLOT}{}",
            r#"
    fn take_out() Slot
        return replace(self, Slot.Empty)

fn main()
    var s = Slot.Full(Res(1))
    let t = s.take_out()
    match s
        Slot.Full(r): println(f"left {r.id}")
        Slot.Empty: println("left empty")
    match t
        Slot.Full(r): keep(r)
        Slot.Empty: println("empty")
    println("end")
"#
        )),
        "left empty\nkept\nclosed 1\nleaving keep\nend",
    );
}

#[test]
fn taking_an_optional_field_hands_its_payload_to_the_keeper() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let h = Holder(1)
    match h.slot.take()
        Some(x): keep(x)
        None: println("none")
    println(f"empty {h.slot.is_none()}")
"#,
        ),
        "kept\nclosed 1\nleaving keep\nempty true\nclosed 11",
    );
}

#[test]
fn removing_a_list_element_hands_it_to_the_keeper() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    match l.remove_at(0)
        Some(x): keep(x)
        None: println("none")
    println(f"left {l.length()}")
"#,
        ),
        "kept\nclosed 1\nleaving keep\nleft 1\nclosed 2",
    );
}

#[test]
fn removing_each_element_in_a_while_loop_hands_every_one_to_the_keeper() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    while true
        match l.remove_at(0)
            Some(x): x.shelve()
            None: break
    println("end")
"#,
        ),
        "shelved\nclosed 1\nleaving shelve\nshelved\nclosed 2\nleaving shelve\nend",
    );
}

#[test]
fn a_match_on_an_owned_local_hands_its_payload_to_the_keeper() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let o Res? = Some(Res(5))
    match o
        Some(r): keep(r)
        None: println("none")
    println("end")
"#,
        ),
        "kept\nclosed 5\nleaving keep\nend",
    );
}

#[test]
fn an_owned_local_whose_payload_a_keeper_took_is_refused_after() {
    let source = program(
        r#"
fn main()
    let o Res? = Some(Res(5))
    match o
        Some(r): keep(r)
        None: println("none")
    println(f"{o.is_some()}")
"#,
    );
    assert_compiler_error(&source, "MER_OWN_003");
}

#[test]
fn a_string_element_handed_to_a_keeping_function_still_runs() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn keep_text(s String) int
    let _kept = List([s])
    return 0

fn main()
    let l = List(["a", "b"])
    keep_text(l[0])
    for s in l
        keep_text(s)
    println(f"{l[0]}{l[1]}")
"#,
        "ab",
    );
}

#[test]
fn a_plain_resource_field_handed_to_a_keeper_is_still_a_funded_copy() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let h = Holder(1)
    keep(h.conn)
    println(f"still {h.conn.id}")
"#,
        ),
        "kept\nleaving keep\nstill 11\nclosed 1\nclosed 11",
    );
}

#[test]
fn lending_a_list_element_and_a_loop_binding_still_runs() {
    assert_heap_guard_output(
        &program(
            r#"
fn look(r Res) int
    return r.id

fn main()
    let l = List([Res(1)])
    println(f"{look(l[0])}")
    for r in l
        println(f"{look(r)}")
"#,
        ),
        "1\n1\nclosed 1",
    );
}

const BAG: &str = r#"
class Bag<T>
    var items List<T>
    var slot T?
    fn init(first T)
        self.items = List([first])
        self.slot = None
    fn stash(x T)
        self.slot = Some(x)
    fn stash_first()
        self.stash(self.items[0])
"#;

/// A generic class's element read at an open type is refused at each
/// instantiation reading it as a resource.
#[test]
fn an_open_typed_element_handed_to_a_keeper_is_refused_at_a_resource_instantiation() {
    let source = program(&format!(
        "{BAG}{}",
        r#"
fn main()
    let b = Bag<Res>(Res(1))
    b.stash_first()
"#
    ));
    assert_refused_at(&source, CODE, "self.stash(self.items[0])");
}

/// The same body at a type that is no resource keeps a shared copy of the
/// element.
#[test]
fn an_open_typed_element_handed_to_a_keeper_runs_at_a_plain_instantiation() {
    assert_heap_guard_output(
        &program(&format!(
            "{BAG}{}",
            r#"
fn main()
    let b = Bag<int>(7)
    b.stash_first()
    println(f"{b.items[0]} {b.slot ?? 0}")
"#
        )),
        "7 7",
    );
}

/// A generic enum whose method returns its restored receiver.
const POCKET: &str = r#"
enum Pocket<T>
    Full(T)
    Empty

    fn take_out() Pocket<T>
        let old = self
        self = Pocket.Empty
        return old
"#;

/// An enum is a resource at an instantiation whose payload is one, so an
/// element of a list of them is borrowed like any resource element.
#[test]
fn a_generic_enum_element_holding_a_resource_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
enum Pocket<T>
    Full(T)
    Empty

fn keep_pocket(p Pocket<Res>) int
    let _kept = [p]
    return 0

fn main()
    var l = List<Pocket<Res>>()
    l.push(Pocket.Full(Res(1)))
    keep_pocket(l[0])
"#,
        "keep_pocket(l[0])",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn an_enum_element_holding_a_resource_handed_to_a_keeper_is_refused() {
    assert_refused(
        &format!(
            "{SLOT}{}",
            r#"
fn keep_slot(s Slot) int
    let _kept = [s]
    return 0

fn main()
    var l = List<Slot>()
    l.push(Slot.Full(Res(1)))
    keep_slot(l[0])
"#
        ),
        "keep_slot(l[0])",
        "l[0]",
        "`match l.remove_at(0)`",
    );
}

/// A generic enum's method returning its restored receiver is refused at
/// the instantiation whose payload is a resource.
#[test]
fn a_generic_restored_receiver_returned_is_refused_at_a_resource_instantiation() {
    assert_refused(
        &format!(
            "{POCKET}{}",
            r#"
fn main()
    var p = Pocket.Full(Res(1))
    let t = p.take_out()
    match t
        Pocket.Full(r): println(f"took {r.id}")
        Pocket.Empty: println("empty")
"#
        ),
        "        return old",
        "'self'",
        "`return replace(self, <new value>)` (use system.memory)",
    );
}

/// The same method at a payload that is no resource returns a copy.
#[test]
fn a_generic_restored_receiver_returned_runs_at_a_plain_instantiation() {
    assert_heap_guard_output(
        &program(&format!(
            "{POCKET}{}",
            r#"
fn main()
    var p = Pocket.Full(7)
    let t = p.take_out()
    match t
        Pocket.Full(n): println(f"took {n}")
        Pocket.Empty: println("empty")
    match p
        Pocket.Full(n): println(f"left {n}")
        Pocket.Empty: println("left empty")
"#
        )),
        "took 7\nleft empty",
    );
}

/// A generic class keeping what its `out` parameter held, by `method`.
fn crate_keeping(method: &str) -> String {
    format!(
        r#"
class Crate<T>
    var items List<T>
    fn init()
        self.items = List<T>()
{method}
    fn count() int
        return self.items.length()
"#
    )
}

/// A generic class's `out` parameter of its type parameter, kept by the
/// body, is refused at the instantiation reading it as a resource.
#[test]
fn a_generic_out_parameter_kept_is_refused_at_a_resource_instantiation() {
    assert_refused(
        &format!(
            "{}{}",
            crate_keeping(
                "    fn swap(c out T, fresh T)\n        self.items.push(c)\n        c = fresh"
            ),
            r#"
fn main()
    var b = Crate<Res>()
    var c = Res(1)
    b.swap(c, Res(2))
"#
        ),
        "        self.items.push(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

/// `replace` on a generic `out` parameter hands the old value over at a
/// resource instantiation: it is the repair, never refused itself.
#[test]
fn replacing_a_generic_out_parameter_runs_at_a_resource_instantiation() {
    assert_heap_guard_output(
        &program(&format!(
            "{}{}",
            crate_keeping(
                "    fn trade(c out T, fresh T)\n        self.items.push(replace(c, fresh))"
            ),
            r#"
fn main()
    var b = Crate<Res>()
    var c = Res(1)
    b.trade(c, Res(2))
    println(f"now {c.id}, {b.count()} kept")
"#
        )),
        "now 2, 1 kept\nclosed 2\nclosed 1",
    );
}

/// Two payloads of one scrutinee the scope owns, handed to one call, end
/// the scrutinee once: each hook runs once, where the keeper's owner ends.
#[test]
fn two_payloads_of_one_owned_scrutinee_handed_to_one_keeper_end_it_once() {
    assert_heap_guard_output(
        &program(
            r#"
enum Two
    Both(Res, Res)
    Neither

fn keep2(a Res, b Res) int
    if true
        let _s = Shelf(slot: Some(a))
        let _t = Shelf(slot: Some(b))
        println("kept two")
    println("leaving keep2")
    return 0

fn main()
    let t = Two.Both(Res(1), Res(2))
    match t
        Two.Both(a, b): keep2(a, b)
        Two.Neither: println("neither")
    println("end")
"#,
        ),
        "kept two\nclosed 2\nclosed 1\nleaving keep2\nend",
    );
}

/// An element of an array, which offers no way to take one out, is still
/// handed to a keeper as a copy the array keeps funding: its hook runs where
/// the array ends.
#[test]
fn an_array_element_handed_to_a_keeper_is_still_a_funded_copy() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let a = [Res(1)]
    keep(a[0])
    println("end")
"#,
        ),
        "kept\nleaving keep\nend\nclosed 1",
    );
}

#[test]
fn an_array_loop_binding_handed_to_a_keeper_is_still_a_funded_copy() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let a = [Res(1), Res(2)]
    for r in a
        keep(r)
    println("end")
"#,
        ),
        "kept\nleaving keep\nkept\nleaving keep\nend\nclosed 1\nclosed 2",
    );
}

/// A map's value read by key is handed over as a copy the map keeps
/// funding: the map offers no way to take one out by the key alone.
#[test]
fn a_map_value_handed_to_a_keeper_is_still_a_funded_copy() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let m = {"a": Res(1)}
    keep(m["a"])
    println("end")
"#,
        ),
        "kept\nleaving keep\nend\nclosed 1",
    );
}

#[test]
fn an_optional_field_handed_through_a_function_value_is_refused() {
    assert_refused(
        r#"
fn main()
    let h = Holder(1)
    let f = keep_opt
    f(h.slot)
"#,
        "f(h.slot)",
        "h.slot",
        "`h.slot.take()`",
    );
}

#[test]
fn an_optional_element_handed_through_a_function_value_is_refused() {
    assert_refused(
        r#"
fn main()
    var slots = List<Res?>()
    slots.push(Some(Res(1)))
    let f = keep_opt
    f(slots[0])
"#,
        "f(slots[0])",
        "slots[0]",
        "`slots[0].take()`",
    );
}

#[test]
fn taking_an_optional_field_through_a_function_value_hands_it_over() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let h = Holder(1)
    let f = keep_opt
    f(h.slot.take())
    println("end")
"#,
        ),
        "kept opt\nclosed 1\nend\nclosed 11",
    );
}

#[test]
fn a_payload_bound_from_an_optional_element_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn main()
    var slots = List<Res?>()
    slots.push(Some(Res(1)))
    match slots[0]
        Some(x): keep(x)
        None: println("none")
"#,
        "Some(x): keep(x)",
        "slots[0]",
        "`match slots[0].take()`",
    );
}

#[test]
fn taking_an_optional_element_before_matching_hands_its_payload_over() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    var slots = List<Res?>()
    slots.push(Some(Res(1)))
    match slots[0].take()
        Some(x): keep(x)
        None: println("none")
    println(f"left {slots.length()} {slots[0].is_none()}")
"#,
        ),
        "kept\nclosed 1\nleaving keep\nleft 1 true",
    );
}

/// An optional field of an element is borrowed from the element: the
/// refusal names what holds it.
#[test]
fn an_optional_field_of_an_element_handed_to_a_keeper_is_refused() {
    let diagnostic = assert_refused_at(
        &program(
            r#"
fn main()
    var hs = List<Holder>()
    hs.push(Holder(1))
    keep_opt(hs[0].slot)
"#,
        ),
        CODE,
        "keep_opt(hs[0].slot)",
    );
    assert!(
        diagnostic
            .message
            .starts_with("'hs[0].slot' is borrowed from 'hs[0]', so 'keep_opt' cannot keep it"),
        "the refusal names the element holding the field: {diagnostic:#?}"
    );
    let help = diagnostic.help.as_deref().unwrap_or_default();
    assert!(help.contains("`hs[0].slot.take()`"), "{diagnostic:#?}");
}

#[test]
fn taking_an_optional_field_of_an_element_hands_it_over() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    var hs = List<Holder>()
    hs.push(Holder(1))
    keep_opt(hs[0].slot.take())
    println(f"empty {hs[0].slot.is_none()}")
"#,
        ),
        "kept opt\nclosed 1\nempty true\nclosed 11",
    );
}

#[test]
fn a_var_bound_to_a_list_element_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    var mine = l[0]
    keep(mine)
"#,
        "keep(mine)",
        "the element 'l[0]'",
        "`match l.remove_at(0)`",
    );
}

#[test]
fn an_alias_of_an_alias_of_a_list_element_handed_to_a_keeper_is_refused() {
    assert_refused(
        r#"
fn main()
    let l = List([Res(1)])
    let a = l[0]
    let b = a
    keep(b)
"#,
        "keep(b)",
        "the element 'l[0]'",
        "`match l.remove_at(0)`",
    );
}

/// Checking an optional field holds a value does not make it the caller's:
/// the hand-over under the check is refused all the same.
#[test]
fn an_optional_field_handed_to_a_keeper_under_a_check_is_refused() {
    assert_refused(
        r#"
fn main()
    let h = Holder(1)
    if h.slot.is_some()
        keep_opt(h.slot)
"#,
        "        keep_opt(h.slot)",
        "h.slot",
        "`h.slot.take()`",
    );
}

#[test]
fn taking_an_optional_field_under_a_check_hands_it_over() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let h = Holder(1)
    if h.slot.is_some()
        keep_opt(h.slot.take())
    println("end")
"#,
        ),
        "kept opt\nclosed 1\nend\nclosed 11",
    );
}

/// An element stored into a tuple and handed on from it reaches the keeper
/// as a funded copy the list keeps holding: its hook runs where the list
/// ends.
///
/// TODO: storing an element read out of a list into a tuple will be refused
/// once a borrowed value cannot be stored; this test then asserts that
/// refusal instead.
#[test]
fn an_element_stored_in_a_tuple_and_handed_to_a_keeper_is_still_a_funded_copy() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let l = List([Res(1)])
    let t = (l[0], 1)
    keep(t.0)
    println("end")
"#,
        ),
        "kept\nleaving keep\nend\nclosed 1",
    );
}

/// The code the help of the refusal of `rest` at `handing` suggests: the
/// text between its first pair of backticks.
fn suggested_code(rest: &str, handing: &str) -> String {
    let diagnostic = assert_refused_at(&program(rest), CODE, handing);
    let help = diagnostic.help.unwrap_or_default();
    let code = help.split('`').nth(1);
    code.unwrap_or_else(|| panic!("the help suggests code: {help}"))
        .to_string()
}

/// The code a refusal of an optional field suggests, copied as it is
/// written, compiles and hands the value over.
#[test]
fn the_suggested_take_compiles_and_runs() {
    let suggested = suggested_code(
        "\nfn main()\n    let h = Holder(1)\n    keep_opt(h.slot)\n",
        "keep_opt(h.slot)",
    );
    assert_heap_guard_output(
        &program(&format!(
            "\nfn main()\n    let h = Holder(1)\n    keep_opt({suggested})\n    println(\"end\")\n"
        )),
        "kept opt\nclosed 1\nend\nclosed 11",
    );
}

/// The code a refusal of a list element suggests, copied as it is written,
/// compiles and hands the element over.
#[test]
fn the_suggested_element_removal_compiles_and_runs() {
    let suggested = suggested_code(
        "\nfn main()\n    let l = List([Res(1), Res(2)])\n    keep(l[0])\n",
        "keep(l[0])",
    );
    assert_heap_guard_output(
        &program(&format!(
            "\nfn main()\n    let l = List([Res(1), Res(2)])\n    {suggested}\n        Some(x): keep(x)\n        None: println(\"none\")\n    println(f\"left {{l.length()}}\")\n"
        )),
        "kept\nclosed 1\nleaving keep\nleft 1\nclosed 2",
    );
}

/// The repair names the function the program's declarations provide for
/// replacing a place's value: a public function taking the parameters of a
/// runtime entry point that overwrites its `out` place, under the name it
/// is declared with.
#[test]
fn the_out_parameter_repair_names_the_declared_replacing_function() {
    let source = r#"use system.io

@element("place, new")
@writes("place")
@stores("new")
@returns_element
runtime "core" fn miri_rt_replace<T>(place out T, new T) T

public fn put_back<T>(place out T, new T) T
    return miri_rt_replace(place, new)

class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn keep(r Res) int
    let _kept = [r]
    return 0

fn swap(c out Res)
    keep(c)

fn main()
    var c = Res(1)
    swap(c)
"#;
    let diagnostic = assert_refused_at(source, CODE, "    keep(c)");
    let help = diagnostic.help.as_deref().unwrap_or_default();
    assert!(
        help.contains("`put_back(c, <new value>)`") && !help.contains("(use "),
        "the repair names the program's own replacing function: {diagnostic:#?}"
    );
}
