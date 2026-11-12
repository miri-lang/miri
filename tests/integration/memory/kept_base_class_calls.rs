// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value held at a base class may be any of its subclasses, so the base is
//! a resource when one of its subclasses is: releasing the value may reach
//! that subclass's drop hook. A vtable slot owning the receiver takes such a
//! binding over, the hook runs where its new owner ends, and a second use is
//! refused. A base whose subclasses run no hook stays a plain counted value.
//! Every test reads the exact order of the hooks' output under the heap
//! guard.

use super::super::utils::*;

const BASES: &str = r#"
struct DerivedShelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn shelve(n int) int
        println(f"base {self.id}")
        return n

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed derived {self.id}")
    fn shelve(n int) int
        if true
            let _shelf = DerivedShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n
"#;

fn with_bases(program: &str) -> String {
    format!("{BASES}\n{program}")
}

/// A base-typed binding holding a subclass with a hook is a resource, so a
/// slot owning the receiver takes it over: the hook runs where the keeper's
/// shelf ends, inside the call.
#[test]
fn a_base_typed_binding_an_owning_slot_takes_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn main()
    let b Base = Derived(1)
    println(f"{b.shelve(1)}")
    println("end")
"#,
        ),
        "shelved\nclosed derived 1\nleaving shelve\n1\nend",
    );
}

/// A base-typed resource binding a slot owning the receiver took over cannot
/// be called through again: the second call is reported where it reads it.
#[test]
fn a_base_typed_binding_handed_twice_to_an_owning_slot_is_reported() {
    let program = with_bases(
        r#"
fn main()
    let b Base = Derived(1)
    println(f"{b.shelve(1)}")
    println(f"{b.shelve(2)}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":29:16");
}

/// A base whose subclasses run no hook is no resource: a slot keeping the
/// receiver lends it, and the binding stays usable.
#[test]
fn a_base_typed_binding_over_hook_free_subclasses_stays_usable() {
    assert_heap_guard_output(
        r#"
struct NoteShelf
    slot Note?

class Note
    public var id int
    fn init(id int)
        self.id = id
    fn note(n int) int
        return self.id + n

class Sticky extends Note
    fn init(id int)
        super.init(id)
    fn note(n int) int
        let _shelf = NoteShelf(slot: Some(self))
        return self.id + n * 10

fn main()
    let s Note = Sticky(1)
    println(f"{s.note(1)}")
    println(f"{s.note(2)}")
"#,
        "11\n21",
    );
}

/// A `for` binding over base-typed elements owns no reference of its own, so
/// a slot owning the receiver is handed a funded copy: the keeper's shelf
/// releases its copy, the base's adapter releases its copy, and the hook runs
/// once, where the list ends.
#[test]
fn a_for_binding_at_a_base_handed_to_an_owning_slot_is_a_funded_copy() {
    assert_heap_guard_output(
        &format!(
            "use system.collections.list\n{}",
            with_bases(
                r#"
fn main()
    var l = List<Base>()
    l.push(Derived(3))
    l.push(Base(4))
    for b in l
        println(f"{b.shelve(1)}")
    println("end")
"#,
            )
        ),
        "shelved\nleaving shelve\n1\nbase 4\n1\nend\nclosed derived 3",
    );
}

/// A pattern's payload binding at a base reads the payload and owns no
/// reference of its own, so a slot owning the receiver is handed a funded
/// copy, and the hook runs once, where the optional ends.
#[test]
fn a_payload_binding_at_a_base_handed_to_an_owning_slot_is_a_funded_copy() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn main()
    let o Base? = Some(Derived(5))
    match o
        Some(b): println(f"{b.shelve(1)}")
        None: println("none")
    println("end")
"#,
        ),
        "shelved\nleaving shelve\n1\nend\nclosed derived 5",
    );
}

/// A base-typed parameter handed to a slot owning the receiver is kept by
/// the function, so the function owns it and hands it on: the keeper
/// releases it where its shelf ends, before the call that handed it over
/// returns.
#[test]
fn a_base_typed_parameter_handed_to_an_owning_slot_is_the_functions() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn run(b Base)
    println(f"{b.shelve(1)}")
    println("after")

fn main()
    run(Derived(1))
    run(Base(2))
    println("end")
"#,
        ),
        "shelved\nclosed derived 1\nleaving shelve\n1\nafter\nbase 2\n1\nafter\nend",
    );
}

/// A base-typed parameter a slot owning the receiver took cannot be read
/// after the call, as a binding of the function's own cannot.
#[test]
fn a_base_typed_parameter_an_owning_slot_took_cannot_be_used_after() {
    assert_compiler_error(
        &with_bases(
            r#"
fn run(b Base)
    println(f"{b.shelve(1)}")
    println(f"{b.shelve(2)}")

fn main()
    run(Derived(1))
"#,
        ),
        "MER_OWN_003",
    );
}

/// An abstract base is a resource when a class extending it is one: a slot
/// owning the receiver takes the binding over, and the hook runs inside the
/// call.
#[test]
fn an_abstract_base_with_a_hooked_subclass_is_released_by_the_keeper() {
    assert_heap_guard_output(
        r#"
struct ShapeShelf
    slot Shape?

abstract class Shape
    public var id int
    fn init(id int)
        self.id = id
    abstract fn shelve(n int) int

class Disc extends Shape
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed disc {self.id}")
    fn shelve(n int) int
        if true
            let _shelf = ShapeShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n

fn main()
    let s Shape = Disc(1)
    println(f"{s.shelve(1)}")
    println("end")
"#,
        "shelved\nclosed disc 1\nleaving shelve\n1\nend",
    );
}

const CHAIN: &str = r#"
struct LeafShelf
    slot Root?

class Root
    public var id int
    fn init(id int)
        self.id = id
    fn shelve(n int) int
        println(f"root {self.id}")
        return n

class Middle extends Root
    fn init(id int)
        super.init(id)

class Leaf extends Middle
    fn init(id int)
        super.init(id)
    fn drop(self)
        println(f"closed leaf {self.id}")
    fn shelve(n int) int
        if true
            let _shelf = LeafShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n
"#;

/// In a three-deep chain where only the leaf runs a hook, the root and the
/// class between are both resources: a binding held at either is taken
/// over by a slot owning the receiver.
#[test]
fn every_class_above_a_hooked_leaf_is_a_resource() {
    assert_heap_guard_output(
        &format!(
            "{CHAIN}\n{}",
            r#"
fn main()
    let r Root = Leaf(1)
    println(f"{r.shelve(1)}")
    let m Middle = Leaf(2)
    println(f"{m.shelve(2)}")
    println("end")
"#
        ),
        "shelved\nclosed leaf 1\nleaving shelve\n1\nshelved\nclosed leaf 2\nleaving shelve\n2\nend",
    );
}

/// A binding held at the class between the root and a hooked leaf cannot be
/// called through again once a slot owning the receiver took it.
#[test]
fn a_binding_at_the_middle_of_a_chain_handed_twice_is_reported() {
    assert_compiler_error(
        &format!(
            "{CHAIN}\n{}",
            r#"
fn main()
    let m Middle = Leaf(2)
    println(f"{m.shelve(1)}")
    println(f"{m.shelve(2)}")
"#
        ),
        "MER_OWN_003",
    );
}

/// A base extended by a generic class running a hook is a resource, so a
/// binding held at the base is refused a second call through a slot owning
/// the receiver.
#[test]
fn a_base_extended_by_a_hooked_generic_class_is_a_resource() {
    assert_compiler_error(
        &with_bases(
            r#"
class Boxed<T> extends Base
    public var item T
    fn init(id int, item T)
        super.init(id)
        self.item = item
    fn drop(self)
        println(f"closed boxed {self.id}")

fn main()
    let b Base = Boxed<int>(1, 7)
    println(f"{b.shelve(1)}")
    println(f"{b.shelve(2)}")
"#,
        ),
        "MER_OWN_003",
    );
}

const MODULE_BASE: &str = concat!(
    "use system.io\n",
    "\n",
    "public class Base\n",
    "    public var id int\n",
    "    fn init(id int)\n",
    "        self.id = id\n",
    "    fn shelve(n int) int\n",
    "        println(f\"base {self.id}\")\n",
    "        return n\n",
);

const MAIN_DERIVED: &str = concat!(
    "use system.io\n",
    "use local.bases.{Base}\n",
    "\n",
    "struct DerivedShelf\n",
    "    slot Base?\n",
    "\n",
    "class Derived extends Base\n",
    "    fn init(id int)\n",
    "        super.init(id)\n",
    "    fn drop(self)\n",
    "        println(f\"closed derived {self.id}\")\n",
    "    fn shelve(n int) int\n",
    "        if true\n",
    "            let _shelf = DerivedShelf(slot: Some(self))\n",
    "            println(\"shelved\")\n",
    "        println(\"leaving shelve\")\n",
    "        return n\n",
    "\n",
);

fn main_with_derived(main: &str) -> String {
    format!("{MAIN_DERIVED}{main}")
}

/// A base declared in one module and extended by a hooked class in another
/// is a resource wherever it is held: a function taking the base owns it,
/// and the keeper releases it inside the call.
#[test]
fn a_base_extended_in_another_module_is_a_resource() {
    let main = main_with_derived(concat!(
        "fn run(b Base)\n",
        "    println(f\"{b.shelve(1)}\")\n",
        "    println(\"after\")\n",
        "\n",
        "fn main()\n",
        "    run(Derived(1))\n",
        "    run(Base(2))\n",
        "    println(\"end\")\n",
    ));
    assert_project_runs_with_output(
        &[("main.mi", &main), ("bases.mi", MODULE_BASE)],
        "shelved\nclosed derived 1\nleaving shelve\n1\nafter\nbase 2\n1\nafter\nend",
    );
}

/// A binding held at a base declared in another module than its hooked
/// subclass is refused a second call through a slot owning the receiver.
#[test]
fn a_binding_at_a_base_extended_in_another_module_handed_twice_is_reported() {
    let main = main_with_derived(concat!(
        "fn main()\n",
        "    let b Base = Derived(1)\n",
        "    println(f\"{b.shelve(1)}\")\n",
        "    println(f\"{b.shelve(2)}\")\n",
    ));
    assert_project_compiler_error(
        &[("main.mi", &main), ("bases.mi", MODULE_BASE)],
        "MER_OWN_003",
    );
}
