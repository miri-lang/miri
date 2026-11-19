// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The copy trait is the trait a module declares `@copy_trait`, found by the
//! marker and never by its name. The marker is valid on a trait declaration
//! only, the trait must declare the `clone() Self` a copy calls, and only one
//! trait may carry it. A program's own trait that happens to share the
//! standard library's name, declared without the marker, is an ordinary
//! trait: a collection does not copy its elements through it.

use super::utils::*;

const CODE: &str = "error[MER_TYP_093]";

/// Assert `code` is refused with MER_TYP_093, the message carrying `message`
/// and the help carrying `help`.
fn assert_copy_trait_refused(code: &str, message: &str, help: &str) {
    assert_compiler_error(code, CODE);
    assert_compiler_error(code, message);
    assert_compiler_error(code, help);
}

#[test]
fn test_copy_trait_on_a_class_is_refused() {
    assert_compiler_error(
        r#"
@copy_trait
class Widget
    let size = 1
"#,
        "Attribute @copy_trait is not valid on a class declaration",
    );
}

#[test]
fn test_copy_trait_on_a_free_function_is_refused() {
    assert_compiler_error(
        r#"
@copy_trait
fn copy_everything()
    let x = 1
"#,
        "Attribute @copy_trait is not valid on a function declaration",
    );
}

#[test]
fn test_copy_trait_on_a_trait_without_clone_is_refused() {
    assert_copy_trait_refused(
        r#"
@copy_trait
trait Duplicable
    fn duplicate() Self

fn main()
    println("x")
"#,
        "'Duplicable' is marked `@copy_trait`, but it declares no `clone() Self`, \
         the method a copy calls",
        "declare `fn clone() Self` in 'Duplicable', or remove `@copy_trait`",
    );
}

#[test]
fn test_copy_trait_on_a_trait_whose_clone_takes_an_argument_is_refused() {
    assert_copy_trait_refused(
        r#"
@copy_trait
trait Duplicable
    fn clone(depth int) Self

fn main()
    println("x")
"#,
        "'Duplicable' is marked `@copy_trait`, but it declares no `clone() Self`, \
         the method a copy calls",
        "declare `fn clone() Self` in 'Duplicable', or remove `@copy_trait`",
    );
}

#[test]
fn test_copy_trait_on_a_trait_inheriting_clone_is_refused() {
    assert_copy_trait_refused(
        r#"
trait Cloning
    fn clone() Self

@copy_trait
trait Duplicable extends Cloning
    fn duplicate() Self

fn main()
    println("x")
"#,
        "'Duplicable' is marked `@copy_trait`, but it declares no `clone() Self`, \
         the method a copy calls",
        "declare `fn clone() Self` in 'Duplicable', or remove `@copy_trait`",
    );
}

#[test]
fn test_copy_trait_on_a_second_trait_is_refused() {
    assert_copy_trait_refused(
        r#"
@copy_trait
trait Duplicable
    fn clone() Self

fn main()
    println("x")
"#,
        "'Duplicable' is marked `@copy_trait`, and 'Cloneable' already is: \
         one trait is the copy trait",
        "remove `@copy_trait` from 'Duplicable'",
    );
}

#[test]
fn test_copy_trait_takes_no_argument() {
    assert_compiler_error(
        r#"
@copy_trait("why")
trait Duplicable
    fn clone() Self
"#,
        "Attribute @copy_trait does not take an argument",
    );
}

/// A module's own private `Cloneable`, declared without the marker beside the
/// standard library's, is an ordinary trait: a resource implementing it is not
/// copied through its `clone`, so copying a list of it is refused as the copy
/// of a resource with no `clone` to copy it with.
#[test]
fn test_a_module_trait_named_like_the_copy_trait_is_not_the_copy_trait() {
    let module = r#"use system.collections.list

private trait Cloneable
    fn clone() Self

private class Res implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn clone() Res
        return Res(self.id + 10)
    fn drop(self)
        println(f"drop {self.id}")

public fn copied() int
    let l = List([Res(1)])
    let c = l.clone()
    return c.length()
"#;
    let main = "use local.k.copies as copies\n\nfn main()\n    println(f\"{copies.copied()}\")\n";
    assert_project_compiler_error(
        &[("main.mi", main), ("k/copies.mi", module)],
        "error[MER_OWN_010]",
    );
}

/// A class implementing the marked trait is copied through its `clone`.
#[test]
fn test_the_marked_trait_copies_through_clone() {
    assert_heap_guard_output(
        r#"
use system.collections.list
use system.memory

class Res implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn clone() Res
        println(f"clone {self.id}")
        return Res(self.id + 10)
    fn drop(self)
        println(f"drop {self.id}")

fn main()
    let l = List([Res(1)])
    let c = l.clone()
    println(f"{c[0].id}")
"#,
        "clone 1\n11\ndrop 11\ndrop 1",
    );
}
