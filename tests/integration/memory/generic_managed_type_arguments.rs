// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference counting for a generic class instantiated at a managed type.
//!
//! A body that still spells its type argument `T` classifies that parameter as
//! unmanaged, so a value stored into a field is never retained and a holder
//! releases a reference it never took. Each instantiation gets its own body and
//! its own drop thunk so the concrete type is what reference counting sees.
//!
//! The values here are built at runtime (`"a" + "b"`) because a string literal
//! is not reference counted, which would hide every one of these defects.

use super::super::utils::*;

#[test]
fn test_generic_class_returning_option_of_managed_is_balanced() {
    assert_heap_guard_ok(
        r#"
use system.collections.list

class Box<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn put(x T)
        self.items.push(x)

    public fn take() T?
        if self.items.is_empty()
            return None
        return self.items.remove_at(0)

fn main()
    var b = Box<String>()
    b.put("a" + "b")
    let v = b.take() ?? "none"
    println(v)
"#,
    );
}

#[test]
fn test_generic_class_stores_managed_value_at_full_ownership() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Box<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn put(x T)
        self.items.push(x)

    public fn take() T?
        if self.items.is_empty()
            return None
        return self.items.remove_at(0)

fn main()
    var b = Box<String>()
    let s = "a" + "b"
    b.put(s)
    let v = b.take() ?? "none"
    println(v)
"#,
        "ab",
    );
}

#[test]
fn test_dropping_a_generic_class_releases_its_managed_elements() {
    assert_heap_guard_ok(
        r#"
use system.collections.list

class Box<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn put(x T)
        self.items.push(x)

fn main()
    var b = Box<String>()
    b.put("a" + "b")
    println("scope end")
"#,
    );
}

#[test]
fn test_queue_of_managed_values_enqueue_and_dequeue_is_balanced() {
    assert_heap_guard_ok(
        r#"
use system.collections.queue

fn main()
    var q = Queue<String>()
    var i = 0
    while i < 5
        q.enqueue("item" + f"{i}")
        i += 1
    while q.length() > 0
        let v = q.dequeue() ?? "none"
        println(v)
"#,
    );
}

#[test]
fn test_stack_of_managed_values_push_and_pop_is_balanced() {
    assert_heap_guard_ok(
        r#"
use system.collections.stack

fn main()
    var s = Stack<String>()
    s.push("a" + "b")
    s.push("c" + "d")
    let t = s.pop() ?? "none"
    println(t)
"#,
    );
}

#[test]
fn test_dropping_a_queue_releases_the_elements_it_still_holds() {
    assert_heap_guard_ok(
        r#"
use system.collections.queue

fn main()
    var q = Queue<String>()
    q.enqueue("a" + "b")
    q.enqueue("c" + "d")
    let v = q.dequeue() ?? "none"
    println(v)
"#,
    );
}
