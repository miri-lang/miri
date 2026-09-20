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
