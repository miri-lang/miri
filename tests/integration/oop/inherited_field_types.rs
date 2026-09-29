// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An ancestor declares each field's type in its own parameters, and the
//! `extends` clause says what those parameters are bound to. A clause that
//! renames, reorders or wraps them means the child reads every inherited field
//! at a type whose name appears nowhere in the child's own declaration, so the
//! binding has to be followed rather than matched by name.
//!
//! Reading the field at the wrong type is not a cosmetic error: at a scalar it
//! prints one type's bits as another, and at a managed type the field is
//! released by the wrong rule — or twice. Strings are built at run time so a
//! missing reference cannot pass for a correct answer.

use super::utils::*;

const SWAPPED: &str = r#"
class Base<A, B>
    left A
    right B

    fn init(left A, right B)
        self.left = left
        self.right = right

class Child<X, Y> extends Base<Y, X>
    fn init(left Y, right X)
        super.init(left, right)
"#;

fn with_swapped(main: &str) -> String {
    format!("{SWAPPED}\n{main}")
}

#[test]
fn a_swapped_extends_clause_reads_each_inherited_field_at_its_bound_type() {
    assert_heap_guard_output(
        &with_swapped(
            r#"
fn main()
    let a = Child<int, float>(1.5, 2)
    println(f"{a.left},{a.right}")
"#,
        ),
        "1.5,2",
    );
}

#[test]
fn a_swapped_extends_clause_releases_a_managed_inherited_field() {
    assert_heap_guard_output(
        &with_swapped(
            r#"
fn main()
    let a = Child<int, String>("h" + "i", 2)
    println(f"{a.left},{a.right}")
"#,
        ),
        "hi,2",
    );
}

#[test]
fn an_inherited_field_returned_from_a_childs_own_method_is_released_once() {
    assert_heap_guard_output(
        r#"
class Base<A, B>
    left A
    right B

    fn init(left A, right B)
        self.left = left
        self.right = right

class Child<X, Y> extends Base<Y, X>
    fn init(left Y, right X)
        super.init(left, right)

    fn get_left() Y
        return self.left

    fn get_right() X
        return self.right

fn main()
    let a = Child<int, String>("h" + "i", 2)
    let l = a.get_left()
    let r = a.get_right()
    println(f"{l},{r}")
"#,
        "hi,2",
    );
}

#[test]
fn writing_a_managed_inherited_field_releases_what_it_replaces() {
    assert_heap_guard_output(
        &with_swapped(
            r#"
fn main()
    var a = Child<int, String>("h" + "i", 2)
    a.left = "z" + "z"
    println(f"{a.left},{a.right}")
"#,
        ),
        "zz,2",
    );
}

#[test]
fn a_swapped_clause_two_levels_deep_still_binds_each_field() {
    assert_heap_guard_output(
        r#"
class Base<A, B>
    left A
    right B
    fn init(left A, right B)
        self.left = left
        self.right = right

class Mid<P, Q> extends Base<Q, P>
    fn init(left Q, right P)
        super.init(left, right)

class Leaf<M, N> extends Mid<N, M>
    fn init(left M, right N)
        super.init(left, right)

fn main()
    let a = Leaf<int, String>(7, "h" + "i")
    println(f"{a.left},{a.right}")
"#,
        "7,hi",
    );
}

#[test]
fn a_clause_that_wraps_the_childs_parameter_binds_the_inherited_field() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Base<A>
    items List<A>

    fn init(items List<A>)
        self.items = items

class Child<U> extends Base<List<U>>
    fn init(items List<List<U>>)
        super.init(items)

fn main()
    var inner = List<String>()
    inner.push("h" + "i")
    var outer = List<List<String>>()
    outer.push(inner)
    let c = Child<String>(outer)
    println(f"{c.items.length()} {c.items[0][0]}")
"#,
        "1 hi",
    );
}

#[test]
fn a_swapped_clause_survives_being_held_in_a_collection() {
    assert_heap_guard_output(
        &with_swapped(
            r#"
use system.collections.list

fn main()
    var xs = List<Child<int, String>>()
    xs.push(Child<int, String>("h" + "i", 2))
    xs.push(Child<int, String>("b" + "ye", 3))
    println(f"{xs.length()} {xs[0].left} {xs[1].right}")
"#,
        ),
        "2 hi 3",
    );
}

const ROTATED: &str = r#"
class Base<A, B, C>
    first A
    second B
    third C

    fn init(first A, second B, third C)
        self.first = first
        self.second = second
        self.third = third
"#;

/// `super` is the ancestor at the arguments the `extends` clause gives it, so
/// `super.init` takes a `Z` first here — handing it the child's `X` would store
/// an `int` in a slot read back as a `float`.
#[test]
fn super_init_is_refused_an_argument_the_clause_binds_elsewhere() {
    assert_compiler_error(
        &format!(
            "{ROTATED}{}",
            r#"
class Child<X, Y, Z> extends Base<Z, X, Y>
    fn init(first X, second Y, third Z)
        super.init(first, second, third)

fn main()
    let c = Child<int, String, float>(2, "h" + "i", 1.5)
    println(f"{c.first}")
"#
        ),
        "expected Z, got X",
    );
}

#[test]
fn super_init_passing_each_argument_where_the_clause_binds_it_runs() {
    assert_heap_guard_output(
        &format!(
            "{ROTATED}{}",
            r#"
class Child<X, Y, Z> extends Base<Z, X, Y>
    fn init(first X, second Y, third Z)
        super.init(third, first, second)

fn main()
    let c = Child<int, String, float>(2, "h" + "i", 1.5)
    println(f"{c.first} {c.second} {c.third}")
"#
        ),
        "1.5 2 hi",
    );
}

const CONCRETE_CLAUSE: &str = r#"
class Base<A>
    first A
    fn init(first A)
        self.first = first
"#;

#[test]
fn super_init_is_refused_a_value_the_concrete_clause_does_not_bind() {
    assert_compiler_error(
        &format!(
            "{CONCRETE_CLAUSE}{}",
            r#"
class Child extends Base<String>
    fn init(n int)
        super.init(n)

fn main()
    let c = Child(2)
    println(c.first)
"#
        ),
        "expected String, got int",
    );
}

/// Reading the field through `super` gives the type the clause binds, not the
/// ancestor's bare parameter: an `int` result is refused naming `String`.
#[test]
fn a_field_read_through_super_has_the_type_the_clause_binds() {
    assert_compiler_error(
        &format!(
            "{CONCRETE_CLAUSE}{}",
            r#"
class Child extends Base<String>
    fn init()
        super.init("x" + "y")
    fn peek() int
        return super.first

fn main()
    let c = Child()
    println(f"{c.peek()}")
"#
        ),
        "expected int, got String",
    );
}

/// An inherited method takes its parameter at the type the clause binds, so a
/// child's own `X` is not a `Y` there either.
#[test]
fn an_inherited_method_is_refused_a_parameter_the_clause_binds_elsewhere() {
    assert_compiler_error(
        r#"
class Base<A, B>
    first A
    second B
    fn init(first A, second B)
        self.first = first
        self.second = second
    fn put(a A)
        self.first = a

class Child<X, Y> extends Base<Y, X>
    fn init(first X, second Y)
        super.init(second, first)
    fn misplace(x X)
        self.put(x)

fn main()
    let c = Child<int, String>(2, "h" + "i")
    c.misplace(3)
    println(f"{c.second}")
"#,
        "expected Y, got X",
    );
}

/// A field read through `super` reads the instance's own field: the child's
/// layout begins with its base's, so the base's index finds it.
#[test]
fn a_field_read_through_super_reads_the_instances_field() {
    assert_heap_guard_output(
        r#"
use system.io

class Base
    first String
    fn init(first String)
        self.first = first

class Child extends Base
    own int
    fn init()
        super.init("x" + "y")
        self.own = 7
    fn peek() String
        return super.first

fn main()
    let c = Child()
    println(f"{c.peek()} {c.own}")
"#,
        "xy 7",
    );
}

/// The same through a generic base's clause: the field is read at the type
/// the clause binds.
#[test]
fn a_field_read_through_super_of_a_generic_base_runs() {
    assert_heap_guard_output(
        &format!(
            "{CONCRETE_CLAUSE}{}",
            r#"
class Child extends Base<String>
    fn init()
        super.init("x" + "y")
    fn peek() String
        return super.first

fn main()
    let c = Child()
    println(c.peek())
"#
        ),
        "xy",
    );
}

/// An inherited field constructed by name is stored at the type the
/// `extends` clause binds it to, so the value handed to it is stored, not
/// converted, and released exactly once.
#[test]
fn an_inherited_field_set_by_name_is_stored_at_the_clause_type() {
    for clause in [
        "class W<T> extends Base<Box<T>>",
        "class W extends Base<Box<int>>",
    ] {
        let construct = if clause.contains("<T>") {
            "W<int>"
        } else {
            "W"
        };
        assert_heap_guard_output(
            &format!(
                r#"
use system.io

class Box<T>
    v T

class Base<P>
    p P

{clause}
    k int

fn main()
    let b = Box<int>(v: 3)
    let w = {construct}(p: b, k: 1)
    println(f"{{w.k}} {{w.p.v}}")
"#
            ),
            "1 3",
        );
    }
}

#[test]
fn sorting_nested_generic_classes_reached_through_extends_orders_them() {
    assert_heap_guard_output(
        r#"
use system.io
use system.ops
use system.collections.list

class Box<T> implements Comparable
    v T
    public fn compare(_other Self) int
        if self.v < 10
            return -1
        return 1

class Holder<T> implements Comparable
    v T
    public fn compare(other Self) int
        if self.v < other.v
            return -1
        return 1

class Base<P> implements Comparable
    p P
    public fn compare(other Self) int
        if self.p < other.p
            return -1
        return 1

class W<T> extends Base<Holder<Holder<Box<T>>>>
    k int

fn mk(s int, k int) Holder<W<int>>
    let b = Box<int>(v: s)
    let h1 = Holder<Box<int>>(v: b)
    let h2 = Holder<Holder<Box<int>>>(v: h1)
    return Holder<W<int>>(v: W<int>(p: h2, k: k))

fn main()
    var l = List<Holder<W<int>>>()
    l.push(mk(3, 1))
    l.push(mk(20, 2))
    l.sort()
    println(f"{l[0].v.k}")
"#,
        "1",
    );
}
