// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bodies a container's runtime thunk calls are the ones the ownership
//! verdict refuses to let keep a value, and no others. Each case pairs the
//! two sides of the one answer: a body a thunk reaches runs when the
//! container asks it, and the same body keeping a value is refused with
//! MER_OWN_006; a body no thunk reaches keeps what it is handed like any
//! method.

use super::super::utils::*;

const BASE_AND_SUB: &str = r#"
use system.collections.set

struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Base) bool
        println("base equals")
        return self.id == other.id
    public fn hash() int
        return 0

class Sub extends Base
    fn init(id int)
        super.init(id)
    public fn equals(other Base) bool
        println("sub equals")
        let _kept = Shelf(slot: Some(other))
        return false
"#;

fn with_base_and_sub(program: &str) -> String {
    format!("{BASE_AND_SUB}\n{program}")
}

/// An override matching its element against the base type is not the
/// equality a container of the subclass matches by, so it may keep what it
/// is handed. A set of the base type matches through the base's own
/// `equals`, called by name, so the override never runs from the set.
#[test]
fn an_override_taking_the_base_type_keeps_and_a_base_set_never_calls_it() {
    assert_heap_guard_output(
        &with_base_and_sub(
            r#"
fn main()
    var s = Set<Base>()
    s.add(Sub(1))
    s.add(Sub(1))
    println(f"{s.length()}")
"#,
        ),
        "base equals\ndropped 1\n1\ndropped 1",
    );
}

/// A set of the subclass has no `equals` thunk for it, since the override
/// does not match a subclass element against another, so it matches the
/// instances themselves and the override never runs.
#[test]
fn a_set_of_the_subclass_never_calls_an_override_taking_the_base_type() {
    assert_heap_guard_output(
        &with_base_and_sub(
            r#"
fn main()
    var s = Set<Sub>()
    s.add(Sub(1))
    s.add(Sub(1))
    println(f"{s.length()}")
"#,
        ),
        "2\ndropped 1\ndropped 1",
    );
}

/// A trait's default `equals` is the body a set calls for a class taking
/// it, compiled under that class.
#[test]
fn a_set_matches_through_a_trait_default_equals() {
    assert_heap_guard_output(
        r#"
use system.collections.set

trait Same
    public fn key() int
    public fn equals(other Self) bool
        println("same")
        return self.key() == other.key()

class Tag implements Same
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn key() int
        return self.id
    public fn hash() int
        return 0

fn main()
    var s = Set<Tag>()
    s.add(Tag(1))
    s.add(Tag(1))
    println(f"{s.length()}")
"#,
        "same\ndropped 1\n1\ndropped 1",
    );
}

/// A trait's default `clone` keeping its receiver is refused at the trait,
/// since a container copies the class taking it through that default.
#[test]
fn a_trait_default_clone_keeping_its_receiver_is_refused_at_the_trait() {
    assert_compiler_error(
        r#"
use system.memory
use system.collections.list

trait Twin extends Cloneable
    public fn clone() Self
        return self

class Tag implements Twin
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")

fn main()
    let xs = List<Tag>([Tag(1)])
    println(f"{xs[0].id}")
"#,
        "'Twin.clone' keeps 'self'",
    );
}

/// An abstract class has no instance of its own to copy, and every concrete
/// class below it declares its own `clone`, so the abstract class's `clone`
/// is no body a container calls: it may keep its receiver, and a list of
/// the concrete class copies through the concrete `clone`.
#[test]
fn an_abstract_cloneable_class_may_keep_in_its_own_clone() {
    assert_heap_guard_output(
        r#"
use system.memory
use system.collections.list

abstract class Base implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn clone() Self
        return self

class Sub extends Base
    fn init(id int)
        super.init(id)
    public fn clone() Self
        println("sub clone")
        return Sub(self.id + 10)

fn main()
    let xs = List<Sub>([Sub(1)])
    let ys = xs.clone()
    println(f"{ys[0].id}")
"#,
        "sub clone\n11\ndropped 11\ndropped 1",
    );
}

/// A sorted container orders through `compare`, lending both elements.
#[test]
fn a_compare_keeping_its_parameter_is_refused() {
    assert_compiler_error(
        r#"
use system.ops

struct Shelf
    slot Tag?

class Tag implements Comparable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn compare(other Tag) int
        let _kept = Shelf(slot: Some(other))
        return 0

fn main()
    println(f"{Tag(1).id}")
"#,
        "'Tag.compare' keeps 'other'",
    );
}

/// A set places a class declaring `equals` by its `hash`, lending the
/// element.
#[test]
fn a_hash_keeping_its_receiver_is_refused() {
    assert_compiler_error(
        r#"
struct Shelf
    slot Tag?

class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        return self.id == other.id
    public fn hash() int
        let _kept = Shelf(slot: Some(self))
        return 0

fn main()
    println(f"{Tag(1).id}")
"#,
        "'Tag.hash' keeps 'self'",
    );
}

/// `==` calls the same `equals` a container does, so a keeping `equals` the
/// program reaches only through the operator is refused alike.
#[test]
fn an_equals_keeping_its_parameter_is_refused_when_only_the_operator_calls_it() {
    assert_compiler_error(
        r#"
struct Shelf
    slot Tag?

class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        let _kept = Shelf(slot: Some(other))
        return true

fn main()
    let a = Tag(1)
    let b = Tag(2)
    println(f"{a == b}")
"#,
        "MER_OWN_006",
    );
}

/// `<` calls the `compare` a sorted container does.
#[test]
fn a_compare_keeping_its_parameter_is_refused_when_only_the_operator_calls_it() {
    assert_compiler_error(
        r#"
use system.ops

struct Shelf
    slot Tag?

class Tag implements Comparable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn compare(other Tag) int
        let _kept = Shelf(slot: Some(other))
        return 0

fn main()
    let a = Tag(1)
    let b = Tag(2)
    println(f"{a < b}")
"#,
        "MER_OWN_006",
    );
}
