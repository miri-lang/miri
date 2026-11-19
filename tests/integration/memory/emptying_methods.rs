// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method empties its receiver when the value it returns is taken out of
//! `self`: by `replace`, by a runtime entry point declared to remove the
//! element a binding read out of `self` at the method's index, or by calling
//! such a method on a place of `self`. The compiler learns it from the body
//! alone, so a collection of the program's own offers a way to take an
//! element out exactly as the standard library's do, and the refusal of
//! handing one of its elements to a keeper names that method as the repair.
//! A method returning an element its receiver still holds empties nothing.

use super::super::utils::*;
use super::places_handed_to_keepers::{program, CODE};

/// A collection of the program's own holding resources, walked by position,
/// with `methods` beside its accessors.
fn bag(methods: &str) -> String {
    format!(
        r#"
class Bag implements Indexed<Res>
    var items [Res]
    var count int
    fn init()
        self.items = List<Res>()
        self.count = 0
    fn put(r Res)
        self.items.push(r)
        self.count = self.count + 1
    public fn length() int: self.count
    public fn element_at(index int) Res: self.items[index]
{methods}
"#
    )
}

/// The runtime declarations a collection of the program's own reaches its
/// list through: one removing the element at an index, one reordering.
const LIST_ENTRY_POINTS: &str = r#"
@writes("l")
@removes("l, index")
runtime "core" fn miri_rt_list_remove(l [Res], index int) bool
@writes("l")
runtime "core" fn miri_rt_list_reverse(l [Res])
@writes("l")
@removes("l")
runtime "core" fn miri_rt_list_clear(l [Res])
"#;

/// Asserts the program of `rest` is refused at the line holding `handing`
/// with exactly `message` and `help`.
fn assert_refused_with(rest: &str, handing: &str, (message, help): (&str, &str)) {
    let diagnostic = assert_refused_at(&program(rest), CODE, handing);
    assert_eq!(diagnostic.message, message, "{diagnostic:#?}");
    assert_eq!(diagnostic.help.as_deref(), Some(help), "{diagnostic:#?}");
}

/// The refusal of keeping what `b.element_at(0)` hands back, when `Bag`
/// offers no method taking an element out.
const NO_TAKING_METHOD: (&str, &str) = (
    "'b.element_at(0)' is borrowed from 'b', which still holds it after the call, so 'keep' cannot keep it",
    "keep a copy, `b.element_at(0).clone()`, when its type implements `Cloneable`: 'b' keeps holding the value the call hands back",
);

/// Hands what `b.element_at(0)` returns to a keeper.
const KEEP_AN_ELEMENT: &str = r#"
fn main()
    let b = Bag()
    b.put(Res(1))
    keep(b.element_at(0))
"#;

#[test]
fn a_method_removing_through_a_lists_emptying_method_is_the_repair() {
    let methods = r#"    fn pull(i int) Res?
        self.count = self.count - 1
        return self.items.remove_at(i)
"#;
    assert_refused_with(
        &format!(
            "{}{}",
            bag(methods),
            r#"
fn main()
    let b = Bag()
    b.put(Res(1))
    for r in b
        keep(r)
"#
        ),
        "        keep(r)",
        (
            "'r' is borrowed from 'b', which the loop walks, so 'keep' cannot keep it",
            "remove the elements instead of walking them: in a `while` loop, `match b.pull(0)`, hand on the element of `Some`, and `break` on `None`",
        ),
    );
}

#[test]
fn a_method_removing_through_a_runtime_entry_point_is_the_repair() {
    let methods = r#"    fn pull(i int) Res?
        let item = self.items[i]
        miri_rt_list_remove(self.items, i)
        self.count = self.count - 1
        return item
"#;
    assert_refused_with(
        &format!("{LIST_ENTRY_POINTS}{}{KEEP_AN_ELEMENT}", bag(methods)),
        "    keep(b.element_at(0))",
        (
            NO_TAKING_METHOD.0,
            "take it out instead, with `b.pull`, which hands it over owned, or keep a copy, `b.element_at(0).clone()`, when its type implements `Cloneable`",
        ),
    );
}

#[test]
fn a_method_returning_an_element_after_reordering_empties_nothing() {
    let methods = r#"    fn pick(i int) Res?
        let item = self.items[i]
        miri_rt_list_reverse(self.items)
        self.count = self.count - 1
        return item
"#;
    assert_refused_with(
        &format!("{LIST_ENTRY_POINTS}{}{KEEP_AN_ELEMENT}", bag(methods)),
        "    keep(b.element_at(0))",
        NO_TAKING_METHOD,
    );
}

#[test]
fn a_getter_returning_an_element_empties_nothing() {
    let methods = r#"    fn peek(i int) Res?
        self.count = self.count + 0
        return self.items[i]
"#;
    assert_refused_with(
        &format!("{}{KEEP_AN_ELEMENT}", bag(methods)),
        "    keep(b.element_at(0))",
        NO_TAKING_METHOD,
    );
}

#[test]
fn a_method_taking_an_optional_element_out_is_the_repair() {
    assert_refused_with(
        r#"
class Rack
    var slots [Res?]
    fn init()
        self.slots = List<Res?>()
        self.slots.push(Some(Res(1)))
    public fn element_at(index int) Res?: self.slots[index]
    fn take_at(i int) Res?
        return self.slots[i].take()

fn main()
    let b = Rack()
    keep_opt(b.element_at(0))
"#,
        "    keep_opt(b.element_at(0))",
        (
            "'b.element_at(0)' is borrowed from 'b', which still holds it after the call, so 'keep_opt' cannot keep it",
            "take it out instead, with `b.take_at`, which hands it over owned, or keep a copy, `b.element_at(0).clone()`, when its type implements `Cloneable`",
        ),
    );
}

/// A collection with no method taking an element out offers no repair, and
/// an element handed to a keeper is a copy the collection keeps funding.
#[test]
fn an_element_of_a_collection_taking_nothing_out_is_still_a_funded_copy() {
    let methods = r#"    fn peek(i int) Res?
        return self.items[i]
"#;
    assert_heap_guard_output(
        &program(&format!(
            "{}{}",
            bag(methods),
            r#"
fn main()
    let b = Bag()
    b.put(Res(1))
    for r in b
        keep(r)
    println("end")
"#
        )),
        "kept\nleaving keep\nend\nclosed 1",
    );
}

/// The element `pull` takes out is the caller's own: its hook runs once,
/// where the keeper that took it over ends, before the bag's other element.
#[test]
fn a_method_taking_out_an_element_by_its_index_hands_it_over() {
    let methods = r#"    fn pull(i int) Res?
        self.count = self.count - 1
        return self.items.remove_at(i)
"#;
    assert_heap_guard_output(
        &program(&format!(
            "{}{}",
            bag(methods),
            r#"
fn main()
    let b = Bag()
    b.put(Res(1))
    b.put(Res(2))
    match b.pull(0)
        Some(x): keep(x)
        None: println("none")
    println(f"left {b.length()}")
"#
        )),
        "kept\nclosed 1\nleaving keep\nleft 1\nclosed 2",
    );
}

/// The program of `methods`, a `Bag` method beside its accessors, keeping
/// what `b.pull(0)` hands back.
fn keeping_a_pull(methods: &str) -> String {
    format!(
        "{LIST_ENTRY_POINTS}{}{}",
        bag(methods),
        r#"
fn main()
    let b = Bag()
    b.put(Res(1))
    b.put(Res(2))
    keep_opt(b.pull(0))
"#
    )
}

/// The refusal of keeping what `b.pull(0)` hands back while `b` holds it.
const PULL_STILL_HELD: (&str, &str) = (
    "'b.pull(0)' is borrowed from 'b', which still holds it after the call, so 'keep_opt' cannot keep it",
    "keep a copy, `b.pull(0).clone()`, when its type implements `Cloneable`: 'b' keeps holding the value the call hands back",
);

#[test]
fn removing_the_element_after_the_one_read_takes_nothing_out() {
    let methods = r#"    fn pull(i int) Res?
        let item = self.items[i]
        miri_rt_list_remove(self.items, i + 1)
        return item
"#;
    assert_refused_with(
        &keeping_a_pull(methods),
        "    keep_opt(b.pull(0))",
        PULL_STILL_HELD,
    );
}

#[test]
fn removing_at_a_literal_after_reading_at_a_parameter_takes_nothing_out() {
    let methods = r#"    fn pull(i int) Res?
        let item = self.items[i]
        miri_rt_list_remove(self.items, 0)
        return item
"#;
    assert_refused_with(
        &keeping_a_pull(methods),
        "    keep_opt(b.pull(0))",
        PULL_STILL_HELD,
    );
}

#[test]
fn a_binding_assigned_after_its_element_is_removed_empties_nothing() {
    let methods = r#"    fn pull(i int) Res?
        var item = self.items[i]
        miri_rt_list_remove(self.items, i)
        item = Res(9)
        return item
"#;
    assert_refused_with(
        &format!("{LIST_ENTRY_POINTS}{}{KEEP_AN_ELEMENT}", bag(methods)),
        "    keep(b.element_at(0))",
        NO_TAKING_METHOD,
    );
}

#[test]
fn clearing_after_reading_an_element_takes_it_out() {
    let methods = r#"    fn drain(i int) Res?
        let item = self.items[i]
        miri_rt_list_clear(self.items)
        return item
"#;
    assert_refused_with(
        &format!("{LIST_ENTRY_POINTS}{}{KEEP_AN_ELEMENT}", bag(methods)),
        "    keep(b.element_at(0))",
        (
            NO_TAKING_METHOD.0,
            "take it out instead, with `b.drain`, which hands it over owned, or keep a copy, `b.element_at(0).clone()`, when its type implements `Cloneable`",
        ),
    );
}

#[test]
fn a_binding_read_before_a_reverse_is_still_borrowed() {
    let methods = r#"    fn pick(i int) Res
        let item = self.items[i]
        miri_rt_list_reverse(self.items)
        return item
"#;
    assert_refused_with(
        &format!(
            "{LIST_ENTRY_POINTS}{}{}",
            bag(methods),
            r#"
fn main()
    let b = Bag()
    b.put(Res(1))
    keep(b.pick(0))
"#
        ),
        "    keep(b.pick(0))",
        (
            "'b.pick(0)' is borrowed from 'b', which still holds it after the call, so 'keep' cannot keep it",
            "keep a copy, `b.pick(0).clone()`, when its type implements `Cloneable`: 'b' keeps holding the value the call hands back",
        ),
    );
}

/// A collection keeping its resources in a list it hands elements out of,
/// and others in a map by `int` key, with the map's removing entry point
/// declared as the standard library declares it.
fn keyed(pull: &str) -> String {
    format!(
        r#"
use system.collections.map

@writes("m")
@key("key")
@matches("m")
@removes("m, key")
runtime "core" fn miri_rt_map_remove(m Map<int, Res>, key int) bool

class Bag
    var items [Res]
    var keyed Map<int, Res>
    fn init()
        self.items = List<Res>()
        self.keyed = Map<int, Res>()
    fn put(r Res)
        self.items.push(r)
    public fn element_at(index int) Res: self.items[index]
{pull}

fn main()
    let b = Bag()
    b.put(Res(1))
    keep(b.element_at(0))
"#
    )
}

#[test]
fn removing_a_map_entry_by_the_key_read_takes_it_out() {
    let pull = r#"    fn pull(k int) Res?
        let item = self.keyed[k]
        miri_rt_map_remove(self.keyed, k)
        return item
"#;
    let diagnostic = assert_refused_at(&program(&keyed(pull)), CODE, "    keep(b.element_at(0))");
    assert_eq!(
        diagnostic.help.as_deref(),
        Some("take it out instead, with `b.pull`, which hands it over owned, or keep a copy, `b.element_at(0).clone()`, when its type implements `Cloneable`"),
        "{diagnostic:#?}"
    );
}

#[test]
fn removing_a_map_entry_by_another_key_takes_nothing_out() {
    let pull = r#"    fn pull(k int) Res?
        let item = self.keyed[k]
        miri_rt_map_remove(self.keyed, 0)
        return item
"#;
    let diagnostic = assert_refused_at(&program(&keyed(pull)), CODE, "    keep(b.element_at(0))");
    assert_eq!(
        diagnostic.help.as_deref(),
        Some("keep a copy, `b.element_at(0).clone()`, when its type implements `Cloneable`: 'b' keeps holding the value the call hands back"),
        "{diagnostic:#?}"
    );
}

#[test]
fn an_enum_method_leaving_its_receiver_empty_runs() {
    assert_runs_with_output(
        r#"
enum Cell
    Empty
    Full(int)

    fn drain() int?
        let held = match self
            Cell.Full(x): Some(x)
            Cell.Empty: None
        self = Cell.Empty
        return held

fn main()
    var c = Cell.Full(3)
    println(f"{c.drain()}")
"#,
        "Some(3)",
    );
}
