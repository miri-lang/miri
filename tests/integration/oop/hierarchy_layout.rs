// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A body compiled for a base class runs on every instance of a class that
//! extends it, so a field sits at the same offset in both: a base that takes
//! part in no virtual dispatch and a subclass that does share one layout.

use super::utils::*;

#[test]
fn test_base_method_reads_its_field_on_a_subclass_that_dispatches() {
    assert_heap_guard_output(
        r#"
trait Tagged
    fn tag() String
        return "t"

class Base
    public var id int

    public fn describe() String
        return f"base {self.id}"

class Child extends Base implements Tagged
    public var extra int

fn main()
    var c = Child(id: 4, extra: 5)
    println(c.describe())
    println(f"{c.id} {c.extra}")
"#,
        "base 4\n4 5",
    );
}

#[test]
fn test_base_drop_hook_releases_a_managed_field_of_a_dispatching_subclass() {
    assert_heap_guard_output(
        r#"
trait Closable
    fn drop(self)
        println("trait drop")

class Base
    public var label String

    public fn drop(self)
        println(f"base drop {self.label}")

class Child extends Base implements Closable
    public var extra int

fn main()
    var c = Child(label: f"held {1 + 1}", extra: 5)
    println(f"child {c.extra}")
"#,
        "child 5\nbase drop held 2",
    );
}

#[test]
fn test_base_method_reads_its_field_through_a_trait_receiver() {
    assert_heap_guard_output(
        r#"
trait Named
    fn name() String
        return "trait"

class Base
    public var id int

    public fn name() String
        return f"base {self.id}"

class Child extends Base implements Named
    public var extra int

fn main()
    let n Named = Child(id: 7, extra: 8)
    println(n.name())
"#,
        "base 7",
    );
}

// The instance is handed to a `Named` parameter rather than bound to a
// `Named` local: releasing a class instance through a trait-typed binding does
// not yet release its managed fields, which is apart from where they sit.
#[test]
fn test_generic_base_method_reads_its_managed_field_through_a_trait_receiver() {
    assert_heap_guard_output(
        r#"
trait Named
    fn name() String
        return "trait"

class Base<T>
    public var value T
    public var label String

    public fn name() String
        return f"base {self.label}"

class Child<T> extends Base<T> implements Named
    public var extra int

fn show(n Named)
    println(n.name())

fn main()
    var c = Child<String>(value: f"v{0}", label: f"l{1}", extra: 2)
    println(c.name())
    println(c.value)
    show(c)
"#,
        "base l1\nv0\nbase l1",
    );
}

#[test]
fn test_trait_default_calls_a_base_method_reading_a_field() {
    assert_heap_guard_output(
        r#"
trait Named
    fn other() String

    fn name() String
        return f"named {self.other()}"

class Base
    public var id int

    public fn other() String
        return f"other {self.id}"

class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(id: 4, extra: 5)
    println(c.name())
    let n Named = Child(id: 6, extra: 7)
    println(n.name())
"#,
        "named other 4\nnamed other 6",
    );
}

#[test]
fn test_trait_default_calls_a_base_method_reading_a_managed_field() {
    assert_heap_guard_output(
        r#"
trait Named
    fn other() String

    fn name() String
        return f"named {self.other()}"

class Base
    public var label String

    public fn other() String
        return f"other {self.label}"

class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(label: f"l{4}", extra: 5)
    println(c.name())
"#,
        "named other l4",
    );
}
