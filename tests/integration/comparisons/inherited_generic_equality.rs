// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic class that inherits `equals` from a generic parent compares by the
//! parent's body compiled for the parent's own type arguments, so `==`, a
//! written `.equals` call and a `Set` all reach one definition.

use super::utils::*;

const INHERITED: &str = r#"
class Base<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Base<T>) bool
        return self.value == other.value

class Child<T> extends Base<T>
    fn init(value T)
        super.init(value)
"#;

fn with_inherited(main: &str) -> String {
    format!("{INHERITED}\n{main}")
}

#[test]
fn equal_operator_reaches_the_parent_equals_at_the_child_instantiation() {
    assert_runs_with_output(
        &with_inherited(
            r#"
fn main()
    let a = Child<String>("PEAR".to_lower())
    let b = Child<String>("PEAR".to_lower())
    let c = Child<String>("FIG".to_lower())
    println(f"{a == b},{a == c},{a != c}")
"#,
        ),
        "true,false,true",
    );
}

#[test]
fn written_equals_call_on_an_inherited_generic_method_compares_by_content() {
    assert_runs_with_output(
        &with_inherited(
            r#"
fn main()
    let a = Child<String>("PEAR".to_lower())
    let b = Child<String>("PEAR".to_lower())
    let c = Child<String>("FIG".to_lower())
    println(f"{a.equals(b)},{a.equals(c)}")
"#,
        ),
        "true,false",
    );
}

#[test]
fn set_of_an_inheriting_generic_class_dedupes_equal_payloads() {
    assert_runs_with_output(
        &format!(
            "use system.collections.set\n{INHERITED}\n{}",
            r#"
fn main()
    var s = Set<Child<String>>()
    s.add(Child<String>("PEAR".to_lower()))
    s.add(Child<String>("PEAR".to_lower()))
    s.add(Child<String>("FIG".to_lower()))
    println(f"{s.length()}")
"#
        ),
        "2",
    );
}

#[test]
fn operator_written_call_and_set_contains_agree_on_an_inherited_equals() {
    assert_runs_with_output(
        &format!(
            "use system.collections.set\n{INHERITED}\n{}",
            r#"
fn main()
    let a = Child<String>("PEAR".to_lower())
    let probe = Child<String>("PEAR".to_lower())
    var s = Set<Child<String>>()
    s.add(a)
    println(f"{s.contains(probe)},{a.equals(probe)},{a == probe}")
"#
        ),
        "true,true,true",
    );
}

#[test]
fn an_int_instantiation_of_an_inheriting_class_compares_by_value() {
    assert_runs_with_output(
        &with_inherited(
            r#"
fn main()
    let a = Child<int>(40 + 2)
    let b = Child<int>(42)
    let c = Child<int>(7)
    println(f"{a == b},{a == c}")
"#,
        ),
        "true,false",
    );
}

/// A non-generic parent compiles one shared `equals`, so the child's type
/// argument names no body of its own and the element thunk must ask for the
/// shared symbol rather than a mangled one nothing defines.
#[test]
fn a_generic_child_of_a_non_generic_parent_asks_the_shared_equals() {
    assert_runs_with_output(
        r#"
class Plain
    value String

    fn init(value String)
        self.value = value

    public fn equals(other Plain) bool
        return self.value == other.value

class Kid<T> extends Plain
    fn init(value String)
        super.init(value)

fn main()
    let a = Kid<int>("PEAR".to_lower())
    let b = Kid<int>("PEAR".to_lower())
    let c = Kid<int>("FIG".to_lower())
    println(f"{a == b},{a == c}")
"#,
        "true,false",
    );
}

#[test]
fn an_abstract_generic_parent_supplies_equals_to_its_child() {
    assert_runs_with_output(
        r#"
abstract class Base<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Base<T>) bool
        return self.value == other.value

    public abstract fn tag() String

class Kid<T> extends Base<T>
    fn init(value T)
        super.init(value)

    public fn tag() String
        return "k"

fn main()
    let a = Kid<String>("PEAR".to_lower())
    let b = Kid<String>("PEAR".to_lower())
    let c = Kid<String>("FIG".to_lower())
    println(f"{a == b},{a == c},{a.tag()}")
"#,
        "true,false,k",
    );
}

#[test]
fn a_child_that_overrides_equals_keeps_its_own_body() {
    assert_runs_with_output(
        r#"
class Base<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Base<T>) bool
        return self.value == other.value

class Child<T> extends Base<T>
    fn init(value T)
        super.init(value)

    public fn equals(other Base<T>) bool
        return false

fn main()
    let a = Child<String>("PEAR".to_lower())
    let b = Child<String>("PEAR".to_lower())
    println(f"{a == b},{a.equals(b)}")
"#,
        "false,false",
    );
}

#[test]
fn a_grandchild_reaches_the_grandparent_equals() {
    assert_runs_with_output(
        r#"
class Base<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Base<T>) bool
        return self.value == other.value

class Middle<T> extends Base<T>
    fn init(value T)
        super.init(value)

class Leaf<T> extends Middle<T>
    fn init(value T)
        super.init(value)

fn main()
    let a = Leaf<String>("PEAR".to_lower())
    let b = Leaf<String>("PEAR".to_lower())
    println(f"{a == b},{a.equals(b)}")
"#,
        "true,true",
    );
}
