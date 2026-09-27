// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which body a class runs for a method its `extends` chain and its traits
//! both supply: a method any class in the chain gives a body wins over a trait
//! default, and a default only fills a method no class in the chain gives one.

use super::utils::*;

const NAMED_WITH_DEFAULT: &str = r#"
trait Named
    fn name() String
        return "trait"
"#;

const BASE_DECLARING_NAME: &str = r#"
class Base
    public var id int

    public fn name() String
        return "base"
"#;

fn program(parts: &[&str]) -> String {
    parts.concat()
}

#[test]
fn test_base_method_beats_a_subclass_trait_default() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            BASE_DECLARING_NAME,
            r#"
class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
"#,
        ]),
        "base",
    );
}

#[test]
fn test_base_method_beats_a_subclass_trait_default_through_the_trait() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            BASE_DECLARING_NAME,
            r#"
class Child extends Base implements Named
    public var extra int

fn main()
    let n Named = Child(id: 1, extra: 2)
    println(n.name())
"#,
        ]),
        "base",
    );
}

#[test]
fn test_base_method_beats_a_grandchild_trait_default() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            BASE_DECLARING_NAME,
            r#"
class Child extends Base
    public var extra int

class Grandchild extends Child implements Named
    public var more int

fn main()
    var g = Grandchild(id: 1, extra: 2, more: 3)
    println(g.name())
    let n Named = Grandchild(id: 4, extra: 5, more: 6)
    println(n.name())
"#,
        ]),
        "base\nbase",
    );
}

#[test]
fn test_subclass_method_beats_its_base_and_the_trait_default() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            BASE_DECLARING_NAME,
            r#"
class Child extends Base implements Named
    public var extra int

    public fn name() String
        return "child"

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
    let n Named = Child(id: 3, extra: 4)
    println(n.name())
"#,
        ]),
        "child\nchild",
    );
}

#[test]
fn test_trait_default_fills_a_method_no_class_declares() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            r#"
class Base
    public var id int

class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
    let n Named = Child(id: 3, extra: 4)
    println(n.name())
"#,
        ]),
        "trait\ntrait",
    );
}

#[test]
fn test_generic_base_method_beats_a_generic_subclass_trait_default() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            r#"
class Base<T>
    public var value T

    public fn name() String
        return "base"

class Child<T> extends Base<T> implements Named
    public var extra int

fn main()
    var c = Child<String>(value: "v", extra: 2)
    println(c.name())
    let n Named = Child<String>(value: "w", extra: 3)
    println(n.name())
"#,
        ]),
        "base\nbase",
    );
}

#[test]
fn test_base_method_satisfies_an_abstract_method_of_a_subclass_trait() {
    assert_heap_guard_output(
        &program(&[
            r#"
trait Named
    fn name() String
"#,
            BASE_DECLARING_NAME,
            r#"
class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
    let n Named = Child(id: 3, extra: 4)
    println(n.name())
"#,
        ]),
        "base\nbase",
    );
}

const CLOSABLE_WITH_DEFAULT_DROP: &str = r#"
trait Closable
    fn drop(self)
        println("trait drop")
"#;

#[test]
fn test_base_drop_hook_beats_a_subclass_trait_default_drop() {
    assert_heap_guard_output(
        &program(&[
            CLOSABLE_WITH_DEFAULT_DROP,
            r#"
class Base
    public var id int

    public fn drop(self)
        println(f"base drop {self.id}")

class Child extends Base implements Closable
    public var extra int

fn main()
    var c = Child(id: 4, extra: 5)
    println(f"child {c.extra}")
"#,
        ]),
        "child 5\nbase drop 4",
    );
}

#[test]
fn test_generic_base_drop_hook_beats_a_generic_subclass_trait_default_drop() {
    assert_heap_guard_output(
        &program(&[
            CLOSABLE_WITH_DEFAULT_DROP,
            r#"
class Base<T>
    public var value T

    public fn drop(self)
        println("base drop")

class Child<T> extends Base<T> implements Closable
    public var extra int

fn main()
    var c = Child<String>(value: "v", extra: 5)
    println(f"child {c.extra}")
"#,
        ]),
        "child 5\nbase drop",
    );
}

#[test]
#[ignore = "releasing a class instance through a trait-typed binding runs no drop hook at all, \
            not even one the class declares itself"]
fn test_base_drop_hook_beats_a_subclass_trait_default_drop_through_the_trait() {
    assert_heap_guard_output(
        &program(&[
            CLOSABLE_WITH_DEFAULT_DROP,
            r#"
class Base
    public var id int

    public fn drop(self)
        println("base drop")

class Child extends Base implements Closable
    public var extra int

fn release(c Closable)
    println("releasing")

fn main()
    release(Child(id: 4, extra: 5))
    println("end")
"#,
        ]),
        "releasing\nbase drop\nend",
    );
}

#[test]
fn test_trait_default_fills_an_abstract_base_method() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            r#"
abstract class Base
    public var id int

    abstract fn name() String

class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
    let n Named = Child(id: 3, extra: 4)
    println(n.name())
"#,
        ]),
        "trait\ntrait",
    );
}

#[test]
fn test_intermediate_abstract_class_body_fills_an_abstract_base_method() {
    assert_heap_guard_output(
        r#"
abstract class Base
    public var id int

    abstract fn name() String

abstract class Mid extends Base
    public fn name() String
        return f"mid {self.id}"

class Leaf extends Mid
    public var extra int

fn main()
    var l = Leaf(id: 5, extra: 6)
    println(l.name())
"#,
        "mid 5",
    );
}

#[test]
fn test_abstract_base_method_no_class_or_trait_fills_is_refused() {
    assert_compiler_error(
        r#"
trait Named
    fn name() String

abstract class Base
    public var id int

    abstract fn name() String

class Child extends Base implements Named
    public var extra int

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
"#,
        "Class 'Child' must implement abstract method 'name' from class 'Base'",
    );
}

#[test]
fn test_first_listed_trait_supplies_a_default_two_traits_share() {
    assert_heap_guard_output(
        r#"
trait First
    fn who() String
        return "first"

trait Second
    fn who() String
        return "second"

class Ab implements First, Second
    public var id int

class Ba implements Second, First
    public var id int

fn main()
    println(Ab(id: 1).who())
    println(Ba(id: 2).who())
    let f First = Ba(id: 3)
    println(f.who())
"#,
        "first\nsecond\nsecond",
    );
}

#[test]
fn test_first_listed_parent_trait_supplies_a_default_two_parents_share() {
    assert_heap_guard_output(
        r#"
trait Left
    fn who() String
        return "left"

trait Right
    fn who() String
        return "right"

trait Both extends Left, Right
    fn id() int

class Thing implements Both
    public var key int

    public fn id() int
        return self.key

fn main()
    println(Thing(key: 1).who())
    let b Both = Thing(key: 2)
    println(b.who())
"#,
        "left\nleft",
    );
}

#[test]
fn test_super_call_from_an_override_reaches_the_base_body() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            r#"
class Base
    public var id int

    public fn name() String
        return f"base {self.id}"

class Child extends Base implements Named
    public var extra int

    public fn name() String
        return f"child+{super.name()}"

fn main()
    var c = Child(id: 1, extra: 2)
    println(c.name())
"#,
        ]),
        "child+base 1",
    );
}

#[test]
fn test_nearest_class_trait_list_supplies_the_default() {
    assert_heap_guard_output(
        r#"
trait Outer
    fn who() String
        return "outer"

trait Inner
    fn who() String
        return "inner"

class Base implements Outer
    public var id int

class Child extends Base implements Inner
    public var extra int

fn main()
    println(Child(id: 1, extra: 2).who())
    let o Outer = Child(id: 3, extra: 4)
    println(o.who())
    println(Base(id: 5).who())
"#,
        "inner\ninner\nouter",
    );
}

#[test]
fn test_subclass_drop_hook_beats_its_base_hook_and_a_trait_default() {
    assert_heap_guard_output(
        &program(&[
            CLOSABLE_WITH_DEFAULT_DROP,
            r#"
class Base
    public var id int

    public fn drop(self)
        println("base drop")

class Child extends Base implements Closable
    public var extra int

    public fn drop(self)
        println(f"child drop {self.id}")

fn main()
    var c = Child(id: 4, extra: 5)
    println(f"child {c.extra}")
"#,
        ]),
        "child 5\nchild drop 4",
    );
}

#[test]
fn test_base_method_whose_signature_differs_from_a_subclass_trait_default_is_refused() {
    assert_compiler_error(
        &program(&[
            NAMED_WITH_DEFAULT,
            r#"
class Base
    public var id int

    public fn name() int
        return 1

class Child extends Base implements Named
    public var extra int

fn main()
    let n Named = Child(id: 1, extra: 2)
    println(n.name())
"#,
        ]),
        "Method 'name' in class 'Child' does not match trait 'Named' signature",
    );
}

#[test]
fn test_abstract_ancestor_body_reaches_a_class_below_a_concrete_intermediate() {
    assert_heap_guard_output(
        r#"
trait Named
    fn name() String

abstract class Top implements Named
    public var id int

    public fn name() String
        return f"top {self.id}"

class Mid extends Top
    public var m int

class Leaf extends Mid
    public var l int

fn main()
    let x = Leaf(id: 1, m: 2, l: 3)
    println(x.name())
    let y = Mid(id: 4, m: 5)
    println(y.name())
    let n Named = Leaf(id: 6, m: 7, l: 8)
    println(n.name())
"#,
        "top 1\ntop 4\ntop 6",
    );
}

const TOP_RE_DECLARED_ABSTRACT: &str = r#"
abstract class Top
    public var id int

    public fn name() String
        return "top"

abstract class Mid extends Top
    abstract fn name() String
"#;

#[test]
fn test_method_re_declared_abstract_hides_the_body_above_it() {
    assert_compiler_error(
        &program(&[
            TOP_RE_DECLARED_ABSTRACT,
            r#"
class Leaf extends Mid
    public var l int

fn main()
    println(Leaf(id: 1, l: 2).name())
"#,
        ]),
        "Class 'Leaf' must implement abstract method 'name' from class 'Mid'",
    );
}

#[test]
fn test_trait_default_fills_a_method_re_declared_abstract() {
    assert_heap_guard_output(
        &program(&[
            NAMED_WITH_DEFAULT,
            TOP_RE_DECLARED_ABSTRACT,
            r#"
class Leaf extends Mid implements Named
    public var l int

fn main()
    println(Leaf(id: 1, l: 2).name())
"#,
        ]),
        "trait",
    );
}
