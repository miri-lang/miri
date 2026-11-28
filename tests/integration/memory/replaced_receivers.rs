// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An enum method that assigns its whole receiver, `self = ...`, writes the
//! place it was called on: the receiver is taken for the call and restored
//! with whatever the method left in it, on a `var` or a `let` binding, on a
//! field, an element or a tuple slot, through a method that calls one that
//! replaces, and through a class method a trait slot reaches. The value
//! replaced is released once, so a counted payload does not leak and a drop
//! hook runs exactly once. A class method, and an enum method nothing can
//! restore a receiver from, is refused (`MER_TYP_091`).

use super::super::utils::*;

const SLOT: &str = r#"
enum Slot
    Full(int)
    Empty

    fn clear()
        self = Slot.Empty

    fn take() Slot
        let old = self
        self = Slot.Empty
        return old

    fn reset()
        self.clear()

    fn show() String
        match self
            Slot.Full(n): return f"full {n}"
            Slot.Empty: return "empty"
"#;

fn with_slot(program: &str) -> String {
    format!("{SLOT}\n{program}")
}

#[test]
fn an_enum_method_assigning_self_writes_the_callers_binding() {
    assert_runs_with_output(
        &with_slot(
            r#"
fn main()
    var s = Slot.Full(3)
    s.clear()
    println(s.show())
"#,
        ),
        "empty",
    );
}

#[test]
fn an_enum_method_returning_its_old_self_leaves_the_replacement_behind() {
    assert_runs_with_output(
        &with_slot(
            r#"
fn main()
    var s = Slot.Full(3)
    let old = s.take()
    println(old.show())
    println(s.show())
"#,
        ),
        "full 3\nempty",
    );
}

#[test]
fn a_method_calling_a_replacing_method_on_self_replaces_the_callers_binding() {
    assert_runs_with_output(
        &with_slot(
            r#"
fn main()
    var s = Slot.Full(3)
    s.reset()
    println(s.show())
"#,
        ),
        "empty",
    );
}

#[test]
fn a_replacing_method_is_callable_on_a_let_binding_and_writes_it() {
    assert_heap_guard_output(
        &with_slot(
            r#"
fn main()
    let s = Slot.Full(3)
    s.clear()
    println(s.show())
"#,
        ),
        "empty",
    );
}

#[test]
fn a_replacing_method_on_a_field_writes_the_field() {
    assert_heap_guard_output(
        &with_slot(
            r#"
class Holder
    var slot Slot

fn main()
    var h = Holder(slot: Slot.Full(3))
    h.slot.clear()
    println(h.slot.show())
"#,
        ),
        "empty",
    );
}

#[test]
fn a_replaced_counted_payload_is_released_once() {
    assert_heap_guard_output(
        r#"
enum Note
    Text(String)
    Blank

    fn rewrite(word String)
        self = Note.Text(f"{word}!")

    fn show() String
        match self
            Note.Text(t): return t
            Note.Blank: return "blank"

fn main()
    var n = Note.Text("hello")
    n.rewrite("bye")
    println(n.show())
    n.rewrite("again")
    println(n.show())
"#,
        "bye!\nagain!",
    );
}

#[test]
fn a_replaced_resource_runs_its_hook_once_and_its_replacement_runs_its_own() {
    assert_heap_guard_output(
        r#"
enum Res
    Open(int)
    Shut

    fn drop(self)
        match self
            Res.Open(n): println(f"closed {n}")
            Res.Shut: println("closed shut")

    fn shut()
        self = Res.Shut

fn main()
    var r = Res.Open(1)
    r.shut()
    println("after")
"#,
        "closed 1\nafter\nclosed shut",
    );
}

#[test]
fn a_resource_method_returning_its_old_self_through_a_local_is_refused() {
    assert_refused_at(
        r#"
enum Res
    Open(int)
    Shut

    fn drop(self)
        println("closed")

    fn take() Res
        let old = self
        self = Res.Shut
        return old

fn main()
    var r = Res.Open(1)
    let old = r.take()
    println("taken")
"#,
        "MER_OWN_008",
        "        let old = self",
    );
}

#[test]
fn a_resource_method_taking_its_old_self_leaves_the_caller_owning_both() {
    assert_heap_guard_output(
        r#"
use system.memory

enum Res
    Open(int)
    Shut

    fn drop(self)
        match self
            Res.Open(n): println(f"closed {n}")
            Res.Shut: println("closed shut")

    fn take() Res
        return replace(self, Res.Shut)

fn main()
    var r = Res.Open(1)
    let old = r.take()
    println("taken")
"#,
        "taken\nclosed 1\nclosed shut",
    );
}

#[test]
fn a_receiver_restored_by_its_method_is_not_consumed_by_the_call() {
    assert_runs_with_output(
        r#"
use system.memory

enum Res
    Open(int)
    Shut

    fn drop(self)
        println("closed")

    fn take() Res
        return replace(self, Res.Shut)

    fn is_open() bool
        match self
            Res.Open(_): return true
            Res.Shut: return false

fn main()
    var r = Res.Open(1)
    let old = r.take()
    println(f"{old.is_open()} {r.is_open()}")
"#,
        "true false",
    );
}

const NOTE: &str = r#"
enum Note
    Text(String)
    Blank

    fn rewrite(word String)
        self = Note.Text(f"{word}!")

    fn show() String
        match self
            Note.Text(t): return t
            Note.Blank: return "blank"
"#;

fn with_note(program: &str) -> String {
    format!("{NOTE}\n{program}")
}

/// A class method a trait slot reaches writes its own field through the
/// replacing enum method, and the class instance keeps the write.
#[test]
fn a_replacing_method_on_a_field_writes_it_through_a_trait_slot() {
    assert_heap_guard_output(
        &with_note(
            r#"
trait Pokable
    fn poke()

class Cell implements Pokable
    var note Note

    fn poke()
        self.note.rewrite("fresh")

fn main()
    var c = Cell(note: Note.Text(f"old{2}"))
    c.poke()
    println(c.note.show())
"#,
        ),
        "fresh!",
    );
}

#[test]
fn a_replacing_method_on_a_field_with_a_counted_payload_writes_the_field() {
    assert_heap_guard_output(
        &with_note(
            r#"
class Holder
    var note Note

fn main()
    var h = Holder(note: Note.Text(f"old{1}"))
    h.note.rewrite("new")
    println(h.note.show())
"#,
        ),
        "new!",
    );
}

#[test]
fn a_replacing_method_on_an_element_with_a_counted_payload_writes_the_element() {
    assert_heap_guard_output(
        &with_note(
            r#"
fn main()
    var xs = [Note.Text(f"a{1}"), Note.Text(f"b{2}")]
    xs[1].rewrite("c")
    println(f"{xs[0].show()} {xs[1].show()}")
"#,
        ),
        "a1 c!",
    );
}

/// The element is written in the list the call names: a copy taken before
/// the call keeps the old element.
#[test]
fn a_replacing_method_on_a_shared_list_element_writes_only_its_own_list() {
    assert_heap_guard_output(
        &with_note(
            r#"
fn main()
    var xs = [Note.Text(f"a{1}")]
    let before = xs
    xs[0].rewrite("c")
    println(f"{before[0].show()} {xs[0].show()}")
"#,
        ),
        "a1 c!",
    );
}

#[test]
fn a_replacing_method_on_a_tuple_slot_with_a_counted_payload_writes_the_slot() {
    assert_heap_guard_output(
        &with_note(
            r#"
fn main()
    var t = (Note.Text(f"a{1}"), 2)
    t.0.rewrite("c")
    println(f"{t.0.show()} {t.1}")
"#,
        ),
        "c! 2",
    );
}

#[test]
fn a_replacing_method_of_a_generic_enum_writes_the_callers_binding() {
    assert_heap_guard_output(
        r#"
enum Opt<T>
    Has(T)
    Nothing

    fn clear()
        self = Opt.Nothing

    fn show() String
        match self
            Opt.Has(_): return "has"
            Opt.Nothing: return "nothing"

fn main()
    var o = Opt.Has(f"x{1}")
    o.clear()
    var xs = [Opt.Has(f"y{2}"), Opt.Has(f"z{3}")]
    xs[0].clear()
    println(f"{o.show()} {xs[0].show()} {xs[1].show()}")
"#,
        "nothing nothing has",
    );
}

/// A lent parameter handed to a replacing method is handed a copy of it, so
/// the caller's value is neither released nor leaked.
#[test]
fn a_lent_parameter_handed_to_a_replacing_method_is_released_once() {
    assert_heap_guard_ok(&format!(
        "{NOTE}\n{}",
        r#"
fn touch(n Note)
    n.rewrite("bye")

fn main()
    let n = Note.Text(f"hello{1}")
    touch(n)
    println(n.show())
"#
    ));
}

#[test]
fn a_receiver_is_replaced_only_by_a_value_of_its_own_type() {
    assert_compiler_error(
        r#"
enum Slot
    Full(int)
    Empty

    fn clear()
        self = 5

fn main()
    var s = Slot.Full(3)
    s.clear()
"#,
        "cannot assign int to Slot",
    );
}

const RECEIVER_REPLACED: &str = "MER_TYP_091";

#[test]
fn a_class_method_assigning_self_is_refused() {
    assert_compiler_error(
        r#"
class Counter
    var n int

    fn restart()
        self = Counter(n: 0)

fn main()
    var c = Counter(n: 4)
    c.restart()
"#,
        RECEIVER_REPLACED,
    );
}

#[test]
fn a_class_method_assigning_self_names_the_method_and_the_field_repair() {
    assert_compiler_error(
        r#"
class Counter
    var n int

    fn restart()
        self = Counter(n: 0)

fn main()
    var c = Counter(n: 4)
    c.restart()
"#,
        "'Counter.restart' assigns its whole receiver",
    );
}

#[test]
fn an_extended_class_method_assigning_self_is_refused() {
    assert_compiler_error(
        r#"
abstract class Base
    var n int

    fn restart()
        self.n = 0

class Child extends Base
    fn restart()
        self = Child(n: 0)

fn main()
    var c = Child(n: 4)
    c.restart()
"#,
        RECEIVER_REPLACED,
    );
}

#[test]
fn a_class_method_reached_through_super_assigning_self_is_refused() {
    assert_compiler_error(
        r#"
abstract class Base
    var n int

    fn restart()
        self = Base(n: 0)

class Child extends Base
    fn restart()
        super.restart()

fn main()
    var c = Child(n: 4)
    c.restart()
"#,
        RECEIVER_REPLACED,
    );
}

#[test]
fn a_method_of_a_class_implementing_a_trait_assigning_self_is_refused() {
    assert_compiler_error(
        r#"
trait Resettable
    fn reset()

class Counter implements Resettable
    var n int

    fn reset()
        self = Counter(n: 0)

fn main()
    var c = Counter(n: 4)
    c.reset()
"#,
        RECEIVER_REPLACED,
    );
}

#[test]
fn a_trait_default_method_assigning_self_is_refused() {
    assert_compiler_error(
        r#"
trait Resettable
    fn reset()
        self = self

class Counter implements Resettable
    var n int

fn main()
    var c = Counter(n: 4)
    c.reset()
"#,
        RECEIVER_REPLACED,
    );
}

#[test]
fn a_class_method_assigning_its_fields_writes_the_callers_binding() {
    assert_runs_with_output(
        r#"
class Counter
    var n int

    fn restart()
        self.n = 0

fn main()
    let c = Counter(n: 4)
    c.restart()
    println(f"{c.n}")
"#,
        "0",
    );
}

#[test]
fn an_enum_equals_assigning_self_is_refused() {
    assert_compiler_error(
        r#"
enum Slot
    Full(int)
    Empty

    fn equals(other Slot) bool
        self = Slot.Empty
        return true

    fn hash() int
        return 1

fn main()
    var a = Slot.Full(1)
    println(f"{a == Slot.Full(2)}")
"#,
        "a container calls it on elements it only lends",
    );
}

#[test]
fn an_enum_drop_hook_calling_a_replacing_method_on_self_is_refused() {
    assert_compiler_error(
        r#"
enum Res
    Open(int)
    Shut

    fn drop(self)
        self.shut()

    fn shut()
        self = Res.Shut

fn main()
    var r = Res.Open(1)
"#,
        "it runs as an instance is built or released",
    );
}
