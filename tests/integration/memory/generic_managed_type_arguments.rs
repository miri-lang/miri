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

#[test]
fn test_unbound_generic_instance_passed_to_a_call_releases_its_field() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn take(t Tagged<String>) bool
    return t.value == "fig"

fn main()
    let h = take(Tagged<String>("FIG".to_lower()))
    println(f"{h}")
"#,
        "true",
    );
}

#[test]
fn test_unbound_generic_instance_used_as_a_receiver_releases_its_field() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v

fn main()
    println(Box<String>("PEAR".to_lower()).get())
"#,
        "pear",
    );
}

#[test]
fn test_set_contains_an_unbound_generic_instance_releases_its_field() {
    assert_runs_with_output(
        r#"
use system.collections.set

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Tagged<T>) bool
        return self.value == other.value

fn main()
    var s = Set<Tagged<String>>()
    s.add(Tagged<String>("PEAR".to_lower()))
    let pear = s.contains(Tagged<String>("PEAR".to_lower()))
    let fig = s.contains(Tagged<String>("FIG".to_lower()))
    println(f"{pear},{fig}")
"#,
        "true,false",
    );
}

#[test]
fn test_unbound_generic_instance_at_a_scalar_keeps_its_value() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v

fn twice(b Box<int>) int
    return b.get() * 2

fn main()
    println(f"{Box<int>(40 + 1).get()},{twice(Box<int>(21))}")
"#,
        "41,42",
    );
}

#[test]
fn test_field_typed_by_the_class_parameter_is_retained_when_returned() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn take(t Tagged<String>) String
    return t.value

fn outlive() String
    let t = Tagged<String>("B".to_lower())
    return take(t)

fn main()
    let s = outlive()
    println(s + "!")
    println(take(Tagged<String>("A".to_lower())))
"#,
        "b!\na",
    );
}

#[test]
fn test_field_typed_by_the_class_parameter_is_retained_inside_a_generic_function() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn take<T>(t Tagged<T>) T
    return t.value

fn pass<T>(a T) T
    return take(Tagged<T>(a))

fn main()
    println(pass("F".to_lower()))
    println(f"{pass(3)}")
"#,
        "f\n3",
    );
}

#[test]
fn test_field_reached_through_a_nested_generic_field_is_retained() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

class Outer<T>
    inner Box<T>

    fn init(v T)
        self.inner = Box<T>(v)

fn deep(o Outer<String>) String
    return o.inner.v

fn outlive() String
    let o = Outer<String>("KEEP".to_lower())
    return deep(o)

fn main()
    println(outlive() + "!")
    println(deep(Outer<String>("DEEP".to_lower())))
"#,
        "keep!\ndeep",
    );
}

#[test]
fn test_field_read_off_a_list_element_is_retained() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn first(a String) String
    var l = List<Tagged<String>>()
    l.push(Tagged<String>(a))
    return l[0].value

fn main()
    println(first("PEAR".to_lower()))
    var l = List<Tagged<String>>()
    l.push(Tagged<String>("FIG".to_lower()))
    println(f"{l[0].value}")
"#,
        "pear\nfig",
    );
}

#[test]
fn test_field_read_off_a_list_element_leaves_the_heap_balanced() {
    assert_heap_guard_ok(
        r#"
use system.collections.list

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn first(a String) String
    var l = List<Tagged<String>>()
    l.push(Tagged<String>(a))
    return l[0].value

fn main()
    println(first("PEAR".to_lower()))
"#,
    );
}

#[test]
fn test_field_read_off_a_list_element_in_a_generic_body_is_retained() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn first<T>(a T) T
    var l = List<Tagged<T>>()
    l.push(Tagged<T>(a))
    return l[0].value

fn main()
    println(first("PEAR".to_lower()))
    println(f"{first(7)}")
"#,
        "pear\n7",
    );
}

#[test]
fn test_storing_into_a_field_declared_at_the_class_parameter_releases_the_old_value() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn main()
    var t = Tagged<String>("OLD".to_lower())
    t.value = "MID".to_lower()
    println(t.value)
"#,
        "mid",
    );
}

#[test]
fn test_storing_through_a_copy_of_a_generic_instance_releases_the_old_value() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn main()
    var t = Tagged<String>("OLD".to_lower())
    var u = t
    u.value = "MID".to_lower()
    println(t.value)
"#,
        "mid",
    );
}

#[test]
fn test_storing_into_an_unmanaged_field_declared_at_the_class_parameter_is_unchanged() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn main()
    var t = Tagged<int>(1)
    t.value = 7
    println(f"{t.value}")
"#,
        "7",
    );
}

#[test]
fn test_storing_into_a_field_declared_at_a_nullable_class_parameter_wraps_the_value() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn main()
    var t = Tagged<String?>(None)
    t.value = "MID".to_lower()
    println(t.value ?? "none")
"#,
        "mid",
    );
}

#[test]
fn test_storing_a_field_into_itself_keeps_the_value_alive() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn main()
    var t = Tagged<String>("OLD".to_lower())
    t.value = t.value
    println(t.value)
"#,
        "old",
    );
}

#[test]
fn test_storing_into_a_field_of_a_list_element_releases_the_old_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

fn main()
    var l = List<Tagged<String>>()
    l.push(Tagged<String>("OLD".to_lower()))
    l[0].value = "NEW".to_lower()
    println(l[0].value)
"#,
        "new",
    );
}

#[test]
fn test_storing_through_a_nested_generic_field_releases_the_old_value() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

class Outer<T>
    inner Box<T>

    fn init(v T)
        self.inner = Box<T>(v)

fn main()
    var o = Outer<String>("OLD".to_lower())
    o.inner.v = "NEW".to_lower()
    println(o.inner.v)
    o.inner = Box<String>("FRESH".to_lower())
    println(o.inner.v)
"#,
        "new\nfresh",
    );
}
