// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Reference counting never frees a cycle, so a store that puts an object back
// into one of its own fields is refused at compile time.
//
// The refused form is the direct one: the stored value is the object itself,
// a closure capturing it, or a variant, constructor call or literal holding
// it, written into a field (or an element of a field) of that same object.
// Each refused shape leaked its allocations before the rule existed. The
// accepted cases are the nearby programs that form no cycle, and the
// rewrite the diagnostic recommends.

use super::super::utils::*;

/// A closure capturing `self`, stored in a field of `self`, is refused.
#[test]
fn test_closure_capturing_self_stored_in_own_field_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Ticker
    count int
    on_tick fn() int
    fn bump() int
        return self.count + 1
    fn wire()
        self.on_tick = fn() int: self.bump()

fn zero() int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick()}")
"#,
        "error[MER_TYP_081]",
    );
}

/// The diagnostic names the captured object, the field, and the rewrite.
#[test]
fn test_reference_cycle_diagnostic_names_the_capture_and_the_field() {
    let code = r#"
use system.io

class Ticker
    count int
    on_tick fn() int
    fn wire()
        self.on_tick = fn() int: self.count

fn zero() int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick()}")
"#;
    assert_compiler_error(
        code,
        "the closure captures 'self' and is stored in 'self.on_tick'",
    );
    assert_compiler_error(code, "pass the object as a parameter instead");
}

/// A closure nested inside the stored closure captures `self` for it.
#[test]
fn test_closure_capturing_self_through_a_nested_closure_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Ticker
    count int
    on_tick fn() int
    fn wire()
        self.on_tick = fn() int: (fn() int: self.count)()

fn zero() int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick()}")
"#,
        "error[MER_TYP_081]",
    );
}

/// An object stored, inside an `Option`, in its own field is refused.
#[test]
fn test_object_stored_in_own_field_through_some_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Node
    value int
    next Option<Node>
    fn link()
        self.next = Some(self)

fn main()
    let n = Node(value: 1, next: None)
    n.link()
    println(f"{n.value}")
"#,
        "the value stored in 'self.next' holds 'self'",
    );
}

/// A struct built around the object, stored in the object's field, is refused.
#[test]
fn test_object_wrapped_in_a_constructed_value_stored_in_own_field_is_refused() {
    assert_compiler_error(
        r#"
use system.io

struct Pair
    a Owner

class Owner
    v int
    p Option<Pair>

fn main()
    var o = Owner(v: 3, p: None)
    o.p = Some(Pair(a: o))
    println(f"{o.v}")
"#,
        "error[MER_TYP_081]",
    );
}

/// A closure capturing the object, stored in an element of its list field, is refused.
#[test]
fn test_closure_stored_in_an_element_of_own_list_field_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

class Owner
    v int
    hooks [fn() int]

fn main()
    var hooks = List<fn() int>()
    hooks.push(fn() int: 0)
    var o = Owner(v: 3, hooks: hooks)
    o.hooks[0] = fn() int: o.v
    println(f"{o.hooks[0]()}")
"#,
        "error[MER_TYP_081]",
    );
}

/// A closure capturing a copy of a field, not the object, forms no cycle.
#[test]
fn test_closure_capturing_a_field_copy_stored_in_own_field_runs() {
    assert_heap_guard_output(
        r#"
use system.io

class Ticker
    count int
    on_tick fn() int
    fn wire()
        let c = self.count
        self.on_tick = fn() int: c + 1

fn zero() int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick()}")
"#,
        "2",
    );
}

/// The recommended rewrite: the closure takes the object as a parameter.
#[test]
fn test_closure_taking_the_object_as_a_parameter_runs_leak_free() {
    assert_heap_guard_output(
        r#"
use system.io

class Ticker
    count int
    on_tick fn(Ticker) int
    fn bump() int
        return self.count + 1
    fn wire()
        self.on_tick = fn(t Ticker) int: t.bump()

fn zero(t Ticker) int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick(t)}")
"#,
        "2",
    );
}

/// Storing a different object in a field forms no cycle.
#[test]
fn test_another_object_stored_in_a_field_runs() {
    assert_heap_guard_output(
        r#"
use system.io

class Owner
    v int
    other Option<Owner>

fn main()
    var a = Owner(v: 1, other: None)
    let b = Owner(v: 2, other: None)
    a.other = Some(b)
    println(f"{a.v}")
"#,
        "1",
    );
}

/// A list is a value: an element store copies it away from the closure's capture.
#[test]
fn test_closure_capturing_a_list_stored_in_its_element_runs() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    var fns = List<fn() int>()
    fns.push(fn() int: 0)
    fns[0] = fn() int: fns.length()
    println(f"{fns[0]()}")
"#,
        "1",
    );
}

/// An array copies on write: an element store copies it away from the
/// closure's capture, so the closure sees the array as it was and no cycle forms.
#[test]
fn test_closure_capturing_an_array_stored_in_its_element_runs() {
    assert_heap_guard_output(
        r#"
use system.io

fn zero() int
    return 0

fn main()
    var a = [zero, zero]
    a[0] = fn() int: a.length()
    println(f"{a[0]()}")
"#,
        "2",
    );
}

/// A struct copies on write: the field store copies it away from the closure's
/// capture, so the closure holds the struct as it was and no cycle forms.
#[test]
fn test_closure_capturing_a_struct_stored_in_its_field_runs() {
    assert_heap_guard_output(
        r#"
use system.io

struct Ticker
    count int
    on_tick fn() int

fn zero() int
    return 0

fn main()
    var t = Ticker(count: 1, on_tick: zero)
    t.on_tick = fn() int: t.count + 1
    t.count = 10
    println(f"{t.on_tick()} {t.count}")
"#,
        "2 10",
    );
}

/// A map copies on write: an entry store copies it away from the closure's capture.
#[test]
fn test_closure_capturing_a_map_stored_in_its_entry_runs() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn zero() int
    return 0

fn main()
    var m Map<String, fn() int> = {"a": zero}
    m["a"] = fn() int: m.length()
    println(f"{m['a']()}")
"#,
        "1",
    );
}

/// Either branch of a conditional holding the object closes the cycle.
#[test]
fn test_object_stored_in_own_field_through_a_conditional_branch_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Node
    value int
    next Option<Node>
    fn link(close bool)
        self.next = if close: Some(self) else: None

fn main()
    let n = Node(value: 1, next: None)
    n.link(true)
    println(f"{n.value}")
"#,
        "the value stored in 'self.next' holds 'self'",
    );
}

/// A store into a field of a field is still a store into the object.
#[test]
fn test_closure_capturing_self_stored_in_a_nested_field_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Hooks
    on_tick fn() int

class Ticker
    count int
    hooks Hooks
    fn wire()
        self.hooks.on_tick = fn() int: self.count

fn zero() int
    return 0

fn main()
    let t = Ticker(count: 1, hooks: Hooks(on_tick: zero))
    t.wire()
    println(f"{t.hooks.on_tick()}")
"#,
        "the closure captures 'self' and is stored in 'self.hooks.on_tick'",
    );
}

/// A closure parameter named like the object binds the name: nothing is captured.
#[test]
fn test_closure_parameter_shadowing_the_object_name_stored_in_its_field_runs() {
    assert_heap_guard_output(
        r#"
use system.io

class Ticker
    count int
    on_tick fn(Ticker) int

fn zero(t Ticker) int
    return 0

fn main()
    var t = Ticker(count: 1, on_tick: zero)
    t.on_tick = fn(t Ticker) int: t.count + 6
    println(f"{t.on_tick(t)}")
"#,
        "7",
    );
}
