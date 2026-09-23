// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A field store inside a class's own method writes through `self`, which is a
// parameter. The object — not the caller — owns what the field holds, so
// overwriting it must release the value it replaces, exactly as the same store
// written from outside the class does.

use super::super::utils::*;

#[test]
fn test_method_field_store_releases_replaced_value() {
    assert_heap_guard_output(
        r#"
class Tagged
    value String

    fn init(value String)
        self.value = value

    fn bump()
        self.value = "MID".to_lower()

fn main()
    var t = Tagged("OLD".to_lower())
    t.bump()
    t.bump()
    t.bump()
    println(t.value)
"#,
        "mid",
    );
}

#[test]
fn test_method_store_into_inherited_field_releases_replaced_value() {
    assert_heap_guard_output(
        r#"
class Base
    public var label String

    fn init(label String)
        self.label = label

class Child extends Base
    fn init(label String)
        super.init(label)

    fn relabel(label String)
        self.label = label

fn main()
    var c = Child("OLD".to_lower())
    c.relabel("MID".to_lower())
    c.relabel("NEW".to_lower())
    println(c.label)
"#,
        "new",
    );
}

#[test]
fn test_generic_method_field_store_releases_replaced_value() {
    assert_heap_guard_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    fn put(v T)
        self.value = v

fn main()
    var t = Tagged<String>("OLD".to_lower())
    t.put("MID".to_lower())
    t.put("NEW".to_lower())
    println(t.value)
"#,
        "new",
    );
}

#[test]
fn test_method_store_of_parameter_keeps_caller_value_alive() {
    assert_heap_guard_output(
        r#"
class Tagged
    value String

    fn init(value String)
        self.value = value

    fn put(v String)
        self.value = v

fn main()
    var t = Tagged("OLD".to_lower())
    let s = "MID".to_lower()
    t.put(s.clone())
    t.put(s.clone())
    println(f"{s} {t.value}")
"#,
        "mid mid",
    );
}

#[test]
fn test_method_store_of_own_field_value_is_balanced() {
    assert_heap_guard_output(
        r#"
class Tagged
    value String

    fn init(value String)
        self.value = value

    fn keep()
        self.value = self.value

fn main()
    var t = Tagged("OLD".to_lower())
    t.keep()
    t.keep()
    println(t.value)
"#,
        "old",
    );
}

#[test]
fn test_method_store_into_list_field_releases_replaced_list() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag
    items [String]

    fn init()
        self.items = List<String>()

    fn reset(s String)
        self.items = List([s])

fn main()
    var b = Bag()
    b.reset("A".to_lower())
    b.reset("B".to_lower())
    println(f"{b.items.length()} {b.items[0]}")
"#,
        "1 b",
    );
}

#[test]
fn test_method_store_into_nested_object_field_releases_replaced_value() {
    assert_heap_guard_output(
        r#"
class Inner
    public var name String

    fn init(name String)
        self.name = name

class Outer
    inner Inner

    fn init()
        self.inner = Inner("OLD".to_lower())

    fn rename(name String)
        self.inner.name = name

    fn replace()
        self.inner = Inner("FRESH".to_lower())

fn main()
    var o = Outer()
    o.rename("MID".to_lower())
    o.rename("NEW".to_lower())
    println(o.inner.name)
    o.replace()
    println(o.inner.name)
"#,
        "new\nfresh",
    );
}

#[test]
fn test_closure_reassigning_captured_value_releases_it_each_call() {
    assert_heap_guard_output(
        r#"
fn main()
    var s = "OLD".to_lower()
    let f = fn()
        s = "MID".to_lower()
        println(s)
    f()
    f()
    println(s)
"#,
        "mid\nmid\nold",
    );
}

#[test]
fn test_closure_returning_reassigned_capture_is_balanced() {
    assert_heap_guard_output(
        r#"
fn main()
    var s = "OLD".to_lower()
    let f = fn(k int) String
        if k > 0
            s = "EARLY".to_lower()
            return s
        s = "LATE".to_lower()
        s
    println(f(1))
    println(f(0))
    println(s)
"#,
        "early\nlate\nold",
    );
}
