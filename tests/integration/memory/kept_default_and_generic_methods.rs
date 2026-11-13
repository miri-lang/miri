// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A trait's default is compiled once for each class inheriting it, and a
//! method of a generic class once for each instantiation, so each copy owns
//! the resource parameters and the receiver it keeps, decided at the types
//! that copy is compiled at: a resource it keeps is its own from the call
//! on, and the hook runs where its new owner ends, inside the call. A call
//! naming the copy and a call through the slot it fills hand it over alike;
//! a value the caller only reads — a field, an element, a loop binding, a
//! payload — is handed over as a funded copy, and its holder releases it.
//! Every test reads the exact order of the hooks' output under the heap
//! guard.

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

fn with_default_takers(program: &str) -> String {
    with_handle(&format!("{DEFAULT_TAKERS}\n{program}"))
}

/// A keeping default owns its parameter in every class inheriting it: the
/// hook runs where the default's shelf ends, before it returns, whether the
/// default is reached through the slot or by a call naming the class.
#[test]
fn a_keeping_trait_default_owns_its_parameter_through_the_slot_and_by_name() {
    assert_heap_guard_output(
        &with_default_takers(
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
    println("end")
"#,
        ),
        "inheritor\nkept by default\nclosed 1\nleaving default take\nafter take\nreader\nread 2\nclosed 2\nafter take\ninheritor\nkept by default\nclosed 3\nleaving default take\nafter static\nend",
    );
}

/// A binding handed to an inherited default by a call naming the class is
/// taken over: a second use is refused.
#[test]
fn a_binding_handed_to_an_inherited_keeping_default_cannot_be_used_after() {
    assert_compiler_error(
        &with_default_takers(
            r#"
fn main()
    let i = Inheritor()
    let h = Handle(4)
    i.take(h)
    println(f"{h.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

/// A field handed to a keeping default is a funded copy: the default's shelf
/// releases its own reference, and the holder releases the field once.
#[test]
fn a_field_handed_to_a_keeping_default_stays_its_holders() {
    assert_heap_guard_output(
        &with_default_takers(
            r#"
class Pocket
    public var h Handle
    fn init(h Handle)
        self.h = h

fn main()
    let i = Inheritor()
    let p = Pocket(Handle(1))
    i.take(p.h)
    println("after field")
    println(f"{p.h.id}")
"#,
        ),
        "inheritor\nkept by default\nleaving default take\nafter field\n1\nclosed 1",
    );
}

/// An element, read by index or bound by a loop, handed to a keeping
/// default is a funded copy: the list still holds it and releases it once.
#[test]
fn an_element_and_a_loop_binding_handed_to_a_keeping_default_stay_the_lists() {
    assert_heap_guard_output(
        &format!(
            "use system.collections.list\n{}",
            with_default_takers(
                r#"
fn main()
    let i = Inheritor()
    var hs List<Handle> = List<Handle>()
    hs.push(Handle(1))
    i.take(hs[0])
    println("after element")
    for h in hs
        i.take(h)
    println("after loop")
    println(f"{hs[0].id}")
"#,
            )
        ),
        "inheritor\nkept by default\nleaving default take\nafter element\nkept by default\nleaving default take\nafter loop\n1\nclosed 1",
    );
}

/// A payload binding handed to a keeping default is a funded copy: the
/// optional releases the payload once, where it ends.
#[test]
fn a_payload_binding_handed_to_a_keeping_default_stays_the_optionals() {
    assert_heap_guard_output(
        &with_default_takers(
            r#"
fn main()
    let i = Inheritor()
    let o Handle? = Some(Handle(5))
    match o
        Some(h): println(f"{i.take(h)}")
        None: println("none")
    println("end")
"#,
        ),
        "inheritor\nkept by default\nleaving default take\n1\nend\nclosed 5",
    );
}

const TRAYS: &str = r#"
struct TrayShelf
    slot Tray?

trait Tray
    fn stow(n int) int
        if true
            let _shelf = TrayShelf(slot: Some(self))
            println("stowed")
        println("leaving stow")
        return n

class Plain implements Tray
    public var id int
    fn init(id int)
        self.id = id

class Lidded implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed lidded {self.id}")
"#;

/// A default keeping its receiver owns it in a class running a hook, and the
/// copy compiled for that class moves it into the shelf, so the hook runs
/// where the shelf ends, through the slot and by name alike; in a class
/// running none the receiver is a plain value, lent, and stays usable.
#[test]
fn a_default_keeping_its_receiver_owns_it_only_where_the_class_is_a_resource() {
    assert_heap_guard_output(
        &format!(
            "{TRAYS}\n{}",
            r#"
fn main()
    let t Tray = Lidded(1)
    println(f"{t.stow(1)}")
    let l = Lidded(2)
    println(f"{l.stow(2)}")
    let p = Plain(3)
    println(f"{p.stow(3)}")
    println(f"{p.stow(4)}")
    println("end")
"#
        ),
        "stowed\nclosed lidded 1\nleaving stow\n1\nstowed\nclosed lidded 2\nleaving stow\n2\nstowed\nleaving stow\n3\nstowed\nleaving stow\n4\nend",
    );
}

/// A resource receiver a keeping default took over by a call naming its
/// class cannot be used again.
#[test]
fn a_resource_receiver_handed_to_a_keeping_default_cannot_be_used_after() {
    assert_compiler_error(
        &format!(
            "{TRAYS}\n{}",
            r#"
fn main()
    let l = Lidded(2)
    println(f"{l.stow(1)}")
    println(f"{l.stow(2)}")
"#
        ),
        "MER_OWN_003",
    );
}

const GENERIC_TAKERS: &str = r#"
trait Taker
    fn take(h Handle) int

class Keeper implements Taker
    fn init()
        println("keeper")
    fn take(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("kept")
        println("leaving keeper")
        return 1

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
"#;

/// A generic class's keeping method owns its parameter at every
/// instantiation: the hook runs where its shelf ends, before it returns,
/// beside a plain keeper and a generic reader reached through adapters.
#[test]
fn a_keeping_generic_class_method_owns_its_parameter_through_the_slot() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{GENERIC_TAKERS}\n{}",
            r#"
fn run(t Taker, h Handle)
    println(f"{t.take(h)}")

fn main()
    run(Keeper(), Handle(1))
    run(Boxed<int>(1), Handle(2))
    run(Boxed<String>("s"), Handle(3))
    run(GKeeper<int>(1), Handle(4))
    let g = GKeeper<String>("g")
    println(f"{g.take(Handle(5))}")
    println("end")
"#
        )),
        "keeper\nkept\nclosed 1\nleaving keeper\n1\nboxed read 2\nclosed 2\n2\nboxed read 3\nclosed 3\n2\ngkept\nclosed 4\nleaving gkeeper\n3\ngkept\nclosed 5\nleaving gkeeper\n3\nend",
    );
}

const HOLDERS: &str = r#"
struct Cell<T>
    item T?

class Holder<T>
    public var tag int
    fn init(tag int)
        self.tag = tag
    fn keep(x T) int
        if true
            let _cell = Cell<T>(item: Some(x))
            println("holder kept")
        println("leaving holder keep")
        return self.tag
"#;

/// A generic class's method keeping a parameter of the class's own type
/// parameter owns it at an instantiation making it a resource, and that copy
/// moves it into the cell, so the hook runs where the cell ends; at a plain
/// instantiation the same method keeps a plain value it is lent.
#[test]
fn a_method_keeping_a_type_parameter_owns_it_only_at_a_resource_instantiation() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{HOLDERS}\n{}",
            r#"
fn main()
    let h = Holder<Handle>(1)
    println(f"{h.keep(Handle(1))}")
    let n = Holder<int>(2)
    let five = 5
    println(f"{n.keep(five)}")
    println(f"{n.keep(five)}")
    println("end")
"#
        )),
        "holder kept\nclosed 1\nleaving holder keep\n1\nholder kept\nleaving holder keep\n2\nholder kept\nleaving holder keep\n2\nend",
    );
}

/// A resource binding a generic class's method took over at a resource
/// instantiation cannot be used again.
#[test]
fn a_binding_a_resource_instantiation_keeps_cannot_be_used_after() {
    assert_compiler_error(
        &with_handle(&format!(
            "{HOLDERS}\n{}",
            r#"
fn main()
    let h = Holder<Handle>(1)
    let x = Handle(3)
    println(f"{h.keep(x)}")
    println(f"{x.id}")
"#
        )),
        "MER_OWN_003",
    );
}

const TWICE_TRAY: &str = r#"
struct TrayShelf
    slot Tray?

trait Tray
    fn stow(n int) int
        if true
            let _first = TrayShelf(slot: Some(self))
            let _second = TrayShelf(slot: Some(self))
            println("stowed twice")
        return n

class Plain implements Tray
    public var id int
    fn init(id int)
        self.id = id
"#;

/// A default storing its receiver twice compiles where every class
/// inheriting it is a plain value: each copy keeps a funded copy.
#[test]
fn a_default_storing_its_receiver_twice_compiles_where_no_inheritor_is_a_resource() {
    assert_heap_guard_output(
        &format!(
            "{TWICE_TRAY}\n{}",
            r#"
fn main()
    let p = Plain(1)
    println(f"{p.stow(1)}")
    println(f"{p.stow(2)}")
"#
        ),
        "stowed twice\n1\nstowed twice\n2",
    );
}

/// The same default is refused once a class inheriting it runs a hook: the
/// copy compiled for that class consumes its receiver at the first store.
#[test]
fn a_default_storing_its_receiver_twice_is_refused_where_an_inheritor_is_a_resource() {
    let program = format!(
        "{TWICE_TRAY}\n{}",
        r#"
class Lidded implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed lidded {self.id}")

fn main()
    let p = Plain(1)
    println(f"{p.stow(1)}")
"#
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Lidded`");
}

const TWICE_HOLDER: &str = r#"
struct Cell<T>
    item T?

class Holder<T>
    public var tag int
    fn init(tag int)
        self.tag = tag
    fn keep(x T) int
        if true
            let _first = Cell<T>(item: Some(x))
            let _second = Cell<T>(item: Some(x))
            println("held twice")
        return self.tag
"#;

/// A generic class's method storing its parameter twice compiles where the
/// program instantiates the class only at plain types.
#[test]
fn a_method_storing_a_type_parameter_twice_compiles_at_a_plain_instantiation() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{TWICE_HOLDER}\n{}",
            r#"
fn main()
    let n = Holder<int>(2)
    println(f"{n.keep(5)}")
"#
        )),
        "held twice\n2",
    );
}

/// The same method is refused once the program instantiates the class at a
/// resource: that copy consumes the parameter at the first store.
#[test]
fn a_method_storing_a_type_parameter_twice_is_refused_at_a_resource_instantiation() {
    let program = with_handle(&format!(
        "{TWICE_HOLDER}\n{}",
        r#"
fn main()
    let n = Holder<int>(2)
    println(f"{n.keep(5)}")
    let h = Holder<Handle>(1)
    println(f"{h.keep(Handle(1))}")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// A copy built only inside a generic function's body is a copy the type
/// checker knows too: `f` called at `Handle` builds `Holder<Handle>`, so a
/// method storing its parameter twice is refused there, naming that copy, as
/// it is at a `Holder<Handle>` the program builds directly.
#[test]
fn a_method_storing_a_type_parameter_twice_is_refused_at_a_copy_built_in_a_generic_function() {
    let program = with_handle(&format!(
        "{TWICE_HOLDER}\n{}",
        r#"
fn f<T>(x T) int
    let h = Holder<T>(7)
    return h.keep(x)

fn main()
    println(f"{f(Handle(1))}")
    println("end")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// The copy is known however many generic functions it is built through:
/// `g` calls `f` at its own parameter, and `g` is called at `Handle`.
#[test]
fn a_method_storing_a_type_parameter_twice_is_refused_at_a_copy_built_two_generic_calls_deep() {
    let program = with_handle(&format!(
        "{TWICE_HOLDER}\n{}",
        r#"
fn f<T>(x T) int
    let h = Holder<T>(7)
    return h.keep(x)

fn g<T>(x T) int
    return f(x)

fn main()
    println(f"{g(Handle(1))}")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// A copy the program reaches only through an `extends` clause is a copy
/// the type checker knows too: `Pinned` declares no parameters and extends
/// `Holder<Handle>`, so calling the method it inherits runs the copy at
/// `Handle`, which is refused, naming that copy, though nothing builds a
/// `Holder<Handle>`.
#[test]
fn a_method_storing_a_type_parameter_twice_is_refused_at_a_copy_an_extends_clause_reaches() {
    let program = with_handle(&format!(
        "{TWICE_HOLDER}\n{}",
        r#"
class Pinned extends Holder<Handle>
    fn init(tag int)
        super.init(tag)

fn main()
    let p = Pinned(3)
    println(f"{p.keep(Handle(1))}")
    println("end")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// A generic class reaches its base at the arguments its `extends` clause
/// restates: `Sub<Handle>` is a `Holder<Handle>`, so the inherited copy at
/// `Handle` is refused though the program writes only the subclass.
#[test]
fn a_method_storing_a_type_parameter_twice_is_refused_at_a_copy_a_generic_subclass_reaches() {
    let program = with_handle(&format!(
        "{TWICE_HOLDER}\n{}",
        r#"
class Sub<U> extends Holder<U>
    fn init(tag int)
        super.init(tag)

fn main()
    let s = Sub<Handle>(3)
    println(f"{s.keep(Handle(1))}")
    println("end")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// The same class extending the holder at a plain type leaves the method
/// storing its parameter twice accepted.
#[test]
fn a_method_storing_a_type_parameter_twice_compiles_at_a_plain_copy_an_extends_clause_reaches() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{TWICE_HOLDER}\n{}",
            r#"
class Pinned extends Holder<int>
    fn init(tag int)
        super.init(tag)

fn main()
    let p = Pinned(3)
    println(f"{p.keep(5)}")
"#
        )),
        "held twice\n3",
    );
}

/// A generic function building the copy only at a plain type leaves the
/// method storing its parameter twice accepted.
#[test]
fn a_method_storing_a_type_parameter_twice_compiles_at_a_plain_copy_built_in_a_generic_function() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{TWICE_HOLDER}\n{}",
            r#"
fn f<T>(x T) int
    let h = Holder<T>(7)
    return h.keep(x)

fn main()
    println(f"{f(5)}")
"#
        )),
        "held twice\n7",
    );
}

/// The same copy storing its parameter once moves it at the store, so the
/// hook runs where the cell holding it ends, inside the call.
#[test]
fn a_method_storing_a_type_parameter_once_moves_it_at_a_copy_built_in_a_generic_function() {
    assert_heap_guard_output(
        &with_handle(
            r#"
struct Cell<T>
    item T?

class Holder<T>
    public var tag int
    fn init(tag int)
        self.tag = tag
    fn keep(x T) int
        if true
            let _first = Cell<T>(item: Some(x))
            println("held once")
        return self.tag

fn f<T>(x T) int
    let h = Holder<T>(7)
    return h.keep(x)

fn main()
    println(f"{f(Handle(1))}")
    println("end")
"#,
        ),
        "held once\nclosed 1\n7\nend",
    );
}

/// A method storing its parameter and then returning it, at a copy built
/// only inside a generic function's body, is refused there: the copy at
/// `Handle` consumes the parameter at the store, so the return reads a value
/// already handed on.
#[test]
fn a_method_storing_a_type_parameter_then_returning_it_is_refused_at_a_copy_built_in_a_generic_function(
) {
    let program = with_handle(
        r#"
struct Cell<T>
    item T?

class Holder<T>
    fn init()
        println("holder")
    fn back(x T) T
        let _c = Cell<T>(item: Some(x))
        return x

fn f<T>(x T) T
    let h = Holder<T>()
    return h.back(x)

fn main()
    let r = f(Handle(4))
    println(f"{r.id}")
    println("end")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// A method storing its parameter in a loop, at a copy built only inside a
/// generic function's body, is refused there: the copy at `Handle` consumes
/// the parameter on the first pass.
#[test]
fn a_method_storing_a_type_parameter_in_a_loop_is_refused_at_a_copy_built_in_a_generic_function() {
    let program = with_handle(
        r#"
struct Cell<T>
    item T?

class Holder<T>
    fn init()
        println("holder")
    fn loop_keep(x T) int
        for i in 0..2
            let _c = Cell<T>(item: Some(x))
        return 2

fn f<T>(x T) int
    let h = Holder<T>()
    return h.loop_keep(x)

fn main()
    println(f"{f(Handle(6))}")
    println("end")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

const CLOSURE_TRAY: &str = r#"
struct Keep
    f fn() int

trait Tray
    fn ident() int
    fn stash() int
        let _k = Keep(f: fn() int: self.ident())
        println("stashed")
        return self.ident()

class Plain implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn ident() int
        return self.id
"#;

/// A default keeping a closure over its receiver and then reading the
/// receiver compiles where every class inheriting it is a plain value, even
/// when a class implementing the trait runs a hook but overrides the
/// default: each copy's closure holds a funded copy.
#[test]
fn a_default_capturing_its_receiver_then_reading_it_compiles_where_no_inheritor_is_a_resource() {
    assert_heap_guard_output(
        &format!(
            "{CLOSURE_TRAY}\n{}",
            r#"
class Sealed implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn ident() int
        return self.id
    fn stash() int
        return self.id
    fn drop(self)
        println(f"closed sealed {self.id}")

fn main()
    let p = Plain(3)
    println(f"{p.stash()}")
    println(f"{p.stash()}")
    let s = Sealed(9)
    println(f"{s.stash()}")
    println("end")
"#
        ),
        "stashed\n3\nstashed\n3\n9\nend\nclosed sealed 9",
    );
}

/// The same default is refused once a class inheriting it runs a hook: the
/// copy compiled for that class hands its receiver to the closure it keeps.
#[test]
fn a_default_capturing_its_receiver_then_reading_it_is_refused_where_an_inheritor_is_a_resource() {
    let program = format!(
        "{CLOSURE_TRAY}\n{}",
        r#"
class Lidded implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn ident() int
        return self.id
    fn drop(self)
        println(f"closed lidded {self.id}")

fn main()
    let p = Plain(1)
    println(f"{p.stash()}")
"#
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Lidded`");
}

/// A default capturing its receiver in a kept closure and not reading it
/// again hands it to the closure in a class running a hook, so the hook runs
/// where the closure's holder ends.
#[test]
fn a_default_capturing_its_receiver_once_hands_it_to_the_closure_at_a_resource() {
    assert_heap_guard_output(
        r#"
struct Keep
    f fn() int

trait Tray
    fn ident() int
    fn stash() int
        let _k = Keep(f: fn() int: self.ident())
        println("stashed")
        return 7

class Plain implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn ident() int
        return self.id

class Lidded implements Tray
    public var id int
    fn init(id int)
        self.id = id
    fn ident() int
        return self.id
    fn drop(self)
        println(f"closed lidded {self.id}")

fn main()
    let l = Lidded(7)
    println(f"{l.stash()}")
    let p = Plain(1)
    println(f"{p.stash()}")
    println("end")
"#,
        "stashed\nclosed lidded 7\n7\nstashed\n7\nend",
    );
}

const CLOSURE_HOLDER: &str = r#"
struct Keep
    f fn() int

class Holder<T>
    public var tag int
    fn init(tag int)
        self.tag = tag
    fn count(_x T) int
        return self.tag
    fn keep(x T) int
        let _k = Keep(f: fn() int: self.count(x))
        println("held in a closure")
        return self.count(x)
"#;

/// A generic class's method capturing its parameter in a kept closure and
/// then reading it compiles where the program instantiates the class only at
/// plain types.
#[test]
fn a_method_capturing_a_type_parameter_then_reading_it_compiles_at_a_plain_instantiation() {
    assert_heap_guard_output(
        &with_handle(&format!(
            "{CLOSURE_HOLDER}\n{}",
            r#"
fn main()
    let n = Holder<int>(2)
    println(f"{n.keep(5)}")
"#
        )),
        "held in a closure\n2",
    );
}

/// The same method is refused once the program instantiates the class at a
/// resource: that copy hands the parameter to the closure it keeps.
#[test]
fn a_method_capturing_a_type_parameter_then_reading_it_is_refused_at_a_resource_instantiation() {
    let program = with_handle(&format!(
        "{CLOSURE_HOLDER}\n{}",
        r#"
fn main()
    let n = Holder<int>(2)
    println(f"{n.keep(5)}")
    let h = Holder<Handle>(1)
    println(f"{h.keep(Handle(1))}")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// A copy built only inside a generic function's body is refused alike.
#[test]
fn a_method_capturing_a_type_parameter_then_reading_it_is_refused_at_a_copy_built_in_a_generic_function(
) {
    let program = with_handle(&format!(
        "{CLOSURE_HOLDER}\n{}",
        r#"
fn f<T>(x T) int
    let h = Holder<T>(7)
    return h.keep(x)

fn main()
    println(f"{f(Handle(1))}")
    println("end")
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "compiled at `Handle`");
}

/// A method capturing its parameter in a kept closure and not reading it
/// again hands it to the closure at a resource instantiation, so the hook
/// runs where the closure's holder ends.
#[test]
fn a_method_capturing_a_type_parameter_once_hands_it_to_the_closure_at_a_resource_instantiation() {
    assert_heap_guard_output(
        &with_handle(
            r#"
struct Keep
    f fn() int

class Holder<T>
    public var tag int
    fn init(tag int)
        self.tag = tag
    fn count(_x T) int
        return self.tag
    fn keep(x T) int
        let _k = Keep(f: fn() int: self.count(x))
        println("held in a closure")
        return self.tag

fn main()
    let n = Holder<int>(2)
    println(f"{n.keep(5)}")
    let h = Holder<Handle>(1)
    println(f"{h.keep(Handle(1))}")
    println("end")
"#,
        ),
        "held in a closure\n2\nheld in a closure\nclosed 1\n1\nend",
    );
}
