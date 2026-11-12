// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether a generic class or struct is a resource is decided per
//! instantiation: an instance is a resource when releasing it may reach a
//! drop hook through a field at the type its arguments give that field. So
//! `Wrap<Handle>` is a resource and `Wrap<int>` is not. A trait, or a base
//! class, is a resource when an instantiation of a generic implementor or
//! subclass the program builds is one. A keeping call takes a resource over,
//! its hook runs where the new owner ends, and a second use is refused. Every
//! test reads the exact order of the hooks' output under the heap guard.

use super::super::utils::*;

const HANDLE: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

class Wrap<T>
    public var item T
    fn init(item T)
        self.item = item
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

const KEEP_WRAP: &str = r#"
struct WrapShelf
    slot Wrap<Handle>?

fn keep(w Wrap<Handle>) int
    if true
        let _shelf = WrapShelf(slot: Some(w))
        println("kept")
    println("leaving keep")
    return 0
"#;

fn with_keep_wrap(program: &str) -> String {
    with_handle(&format!("{KEEP_WRAP}\n{program}"))
}

/// A `Wrap<Handle>` holds a resource at its field's instantiated type, and
/// a keeping function treats it as it treats a plain class holding one: the
/// call is lent the value and retains it, and the hook runs once, where the
/// last holder ends.
#[test]
fn a_resource_instantiation_a_function_keeps_is_released_by_its_last_holder() {
    assert_heap_guard_output(
        &with_keep_wrap(
            r#"
fn main()
    let w = Wrap<Handle>(Handle(1))
    keep(w)
    println("after keep")
"#,
        ),
        "kept\nleaving keep\nafter keep\nclosed 1",
    );
}

/// A `Wrap<Handle>` a keeping function took over cannot be handed on again:
/// the second use is reported where it reads the binding.
#[test]
fn a_resource_instantiation_handed_twice_to_a_keeping_function_is_reported() {
    let program = with_keep_wrap(
        r#"
fn main()
    let w = Wrap<Handle>(Handle(1))
    keep(w)
    keep(w)
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":29:10");
}

/// A `Wrap<int>` holds no resource, so a function keeping it is lent it and
/// the binding stays usable.
#[test]
fn a_plain_instantiation_handed_twice_to_a_keeping_function_stays_usable() {
    assert_heap_guard_output(
        &with_handle(
            r#"
struct IntShelf
    slot Wrap<int>?

fn keep_int(w Wrap<int>) int
    let _shelf = IntShelf(slot: Some(w))
    return w.item

fn main()
    let w = Wrap<int>(5)
    println(f"{keep_int(w)}")
    println(f"{keep_int(w)}")
"#,
        ),
        "5\n5",
    );
}

/// A resource nested two instantiations deep is still reached: a
/// `Wrap<Wrap<Handle>>` a keeping function took cannot be handed on again.
#[test]
fn a_nested_resource_instantiation_handed_twice_to_a_keeping_function_is_reported() {
    assert_compiler_error(
        &with_handle(
            r#"
struct NestedShelf
    slot Wrap<Wrap<Handle>>?

fn keep_nested(w Wrap<Wrap<Handle>>) int
    if true
        let _shelf = NestedShelf(slot: Some(w))
        println("kept")
    println("leaving keep")
    return 0

fn main()
    let w = Wrap<Wrap<Handle>>(Wrap<Handle>(Handle(2)))
    keep_nested(w)
    keep_nested(w)
"#,
        ),
        "MER_OWN_003",
    );
}

/// A collection argument runs no hook, so `Wrap<List<int>>` is no resource:
/// a function keeping it is lent it and the binding stays usable.
#[test]
fn a_collection_instantiation_handed_twice_to_a_keeping_function_stays_usable() {
    assert_heap_guard_output(
        &with_handle(
            r#"
use system.collections.list

struct ListShelf
    slot Wrap<List<int>>?

fn keep_list(w Wrap<List<int>>) int
    let _shelf = ListShelf(slot: Some(w))
    return w.item.length()

fn main()
    let w = Wrap<List<int>>(List([1, 2]))
    println(f"{keep_list(w)}")
    println(f"{keep_list(w)}")
"#,
        ),
        "2\n2",
    );
}

const PAIR: &str = r#"
struct Pair<T>
    first T
    second int

struct PairShelf
    slot Pair<Handle>?

fn keep_pair(p Pair<Handle>) int
    if true
        let _shelf = PairShelf(slot: Some(p))
        println("kept")
    println("leaving keep")
    return 0
"#;

/// A generic struct holding a resource at its instantiated field type is a
/// resource: a `Pair<Handle>` a function keeps is released once, where its
/// last holder ends, and a second use is refused.
#[test]
fn a_generic_struct_at_a_resource_handed_twice_to_a_keeping_function_is_reported() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{PAIR}\n{}",
            r#"
fn main()
    let p = Pair<Handle>(first: Handle(3), second: 4)
    keep_pair(p)
    println("after keep")
"#,
        )),
        "kept\nleaving keep\nafter keep\nclosed 3",
    );
    let program = with_handle(&format!(
        "{PAIR}\n{}",
        r#"
fn main()
    let p = Pair<Handle>(first: Handle(3), second: 4)
    keep_pair(p)
    keep_pair(p)
"#,
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":33:15");
}

/// A list element is the list's, so handing a `Wrap<Handle>` element to a
/// keeping function hands over a funded copy: the list keeps its own, and the
/// hook runs once, where the list ends.
#[test]
fn a_resource_instantiation_element_handed_to_a_keeping_function_is_a_funded_copy() {
    assert_heap_guard_output(
        &format!(
            "use system.collections.list\n{}",
            with_keep_wrap(
                r#"
fn main()
    var l = List<Wrap<Handle>>()
    l.push(Wrap<Handle>(Handle(4)))
    for w in l
        keep(w)
    keep(l[0])
    println("end")
"#,
            )
        ),
        "kept\nleaving keep\nkept\nleaving keep\nend\nclosed 4",
    );
}

const SHELVING: &str = r#"
struct TraitShelf
    slot Shelving?

trait Shelving
    fn shelve(n int) int

class Box<T> implements Shelving
    public var item T
    fn init(item T)
        self.item = item
    fn shelve(n int) int
        if true
            let _shelf = TraitShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n

fn keep_shelving(s Shelving) int
    if true
        let _shelf = TraitShelf(slot: Some(s))
        println("kept")
    println("leaving keep")
    return 0
"#;

fn with_shelving(program: &str) -> String {
    with_handle(&format!("{SHELVING}\n{program}"))
}

/// A trait implemented by a generic class the program builds at a resource
/// is a resource, so a function keeping a value held at the trait takes it
/// over: the hook runs where the shelf ends, inside the call.
#[test]
fn a_trait_binding_over_a_resource_instantiation_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_shelving(
            r#"
fn main()
    let s Shelving = Box<Handle>(Handle(5))
    keep_shelving(s)
    println("after keep")
"#,
        ),
        "kept\nclosed 5\nleaving keep\nafter keep",
    );
}

/// A trait binding over a resource instantiation a keeping function took
/// over cannot be handed on again.
#[test]
fn a_trait_binding_over_a_resource_instantiation_handed_twice_is_reported() {
    let program = with_shelving(
        r#"
fn main()
    let s Shelving = Box<Handle>(Handle(5))
    keep_shelving(s)
    keep_shelving(s)
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":43:19");
}

/// A trait whose generic implementor the program builds only at a plain
/// type is no resource: a keeping function is lent the binding, which stays
/// usable.
#[test]
fn a_trait_binding_over_plain_instantiations_stays_usable() {
    assert_heap_guard_output(
        &with_shelving(
            r#"
fn main()
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#,
        ),
        "kept\nleaving keep\nkept\nleaving keep\nend",
    );
}

/// A slot whose body keeps its receiver is filled by a class holding the
/// resource in a field, which runs no hook of its own, so the slot lends the
/// receiver, as it lends a plain class holding one: the binding stays
/// usable, and the hook runs once, where the binding ends.
#[test]
fn a_slot_keeping_a_resource_instantiation_lends_it_like_a_plain_holder() {
    assert_heap_guard_output(
        &with_shelving(
            r#"
fn main()
    let s Shelving = Box<Handle>(Handle(5))
    println(f"{s.shelve(1)}")
    println(f"{s.shelve(2)}")
"#,
        ),
        "shelved\nleaving shelve\n1\nshelved\nleaving shelve\n2\nclosed 5",
    );
}

/// A `for` binding over trait-typed elements owns no reference of its own,
/// so a keeping function is handed a funded copy of a resource
/// instantiation: the list keeps its own, and the hook runs once, where the
/// list ends.
#[test]
fn a_for_binding_over_a_resource_instantiation_handed_to_a_keeper_is_a_funded_copy() {
    assert_heap_guard_output(
        &format!(
            "use system.collections.list\n{}",
            with_shelving(
                r#"
fn main()
    var l = List<Shelving>()
    l.push(Box<Handle>(Handle(9)))
    for s in l
        keep_shelving(s)
    println("end")
"#,
            )
        ),
        "kept\nleaving keep\nend\nclosed 9",
    );
}

const BASES: &str = r#"
struct BaseShelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id

class Holder<T> extends Base
    public var item T
    fn init(id int, item T)
        super.init(id)
        self.item = item

fn keep_base(b Base) int
    if true
        let _shelf = BaseShelf(slot: Some(b))
        println("kept")
    println("leaving keep")
    return 0
"#;

fn with_bases(program: &str) -> String {
    with_handle(&format!("{BASES}\n{program}"))
}

/// A base class extended by a generic subclass the program builds at a
/// resource is a resource: a function keeping a base-typed binding takes it
/// over, and a second use is refused.
#[test]
fn a_base_binding_over_a_resource_instantiation_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn main()
    let b Base = Holder<Handle>(1, Handle(7))
    keep_base(b)
    println("after keep")
"#,
        ),
        "kept\nclosed 7\nleaving keep\nafter keep",
    );
    let program = with_bases(
        r#"
fn main()
    let b Base = Holder<Handle>(1, Handle(7))
    keep_base(b)
    keep_base(b)
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":40:15");
}

/// A base class whose generic subclass the program builds only at a plain
/// type is no resource: a keeping function is lent a base-typed binding,
/// which stays usable.
#[test]
fn a_base_binding_over_plain_instantiations_stays_usable() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn main()
    let b Base = Holder<int>(1, 8)
    keep_base(b)
    keep_base(b)
    println("end")
"#,
        ),
        "kept\nleaving keep\nkept\nleaving keep\nend",
    );
}

/// A base is a resource because a value held at it may be the subclass
/// holding one; a sibling subclass holding none is still no resource, so a
/// keeping function is lent it and the binding stays usable.
#[test]
fn a_sibling_of_a_resource_instantiation_stays_usable() {
    assert_heap_guard_output(
        &with_bases(
            r#"
class Plain extends Base
    fn init(id int)
        super.init(id)

struct PlainShelf
    slot Plain?

fn keep_plain(p Plain) int
    let _shelf = PlainShelf(slot: Some(p))
    return p.id

fn main()
    let b Base = Holder<Handle>(1, Handle(7))
    keep_base(b)
    let p = Plain(2)
    println(f"{keep_plain(p)}")
    println(f"{keep_plain(p)}")
"#,
        ),
        "kept\nclosed 7\nleaving keep\n2\n2",
    );
}
