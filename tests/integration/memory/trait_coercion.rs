// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A class value already held by a binding, handed on at a trait or abstract
//! type: the new binding shares it, so each holder releases its own reference.

use super::super::utils::*;

const NAMED: &str = "
trait Named
    fn label() String

class Bag implements Named
    var tag String
    fn label() String
        return self.tag
";

fn program(body: &str) -> String {
    format!("{NAMED}\n{body}")
}

#[test]
fn test_a_class_value_bound_to_a_trait_typed_let_is_shared() {
    assert_heap_guard_output(
        &program(
            "
fn main()
    let b = Bag(tag: \"bag\")
    let n Named = b
    println(f\"{n.label()} {b.label()}\")
",
        ),
        "bag bag",
    );
}

#[test]
fn test_a_class_value_bound_to_an_abstract_typed_let_is_shared() {
    assert_heap_guard_output(
        "
abstract class Shape
    fn area() int

class Square extends Shape
    var name String
    var side int
    fn area() int
        return self.side * self.side

fn main()
    let s = Square(name: \"sq\", side: 3)
    let shape Shape = s
    println(f\"{shape.area()} {s.name}\")
",
        "9 sq",
    );
}

#[test]
fn test_a_class_value_returned_at_a_trait_type_is_shared() {
    assert_heap_guard_output(
        &program(
            "
fn named(b Bag) Named
    return b

fn implicit(b Bag) Named: b

fn built_here() Named
    let b = Bag(tag: \"local\")
    return b

fn main()
    let n = named(Bag(tag: \"named\"))
    let m = implicit(Bag(tag: \"implicit\"))
    println(f\"{n.label()} {m.label()} {built_here().label()}\")
",
        ),
        "named implicit local",
    );
}

#[test]
fn test_a_class_value_assigned_to_a_trait_typed_var_is_shared() {
    assert_heap_guard_output(
        &program(
            "
fn main()
    let a = Bag(tag: \"a\")
    let b = Bag(tag: \"b\")
    var n Named = a
    n = b
    println(f\"{n.label()} {a.label()} {b.label()}\")
",
        ),
        "b a b",
    );
}

#[test]
fn test_a_class_value_stored_in_a_trait_typed_field_is_shared() {
    assert_heap_guard_output(
        &program(
            "
class Holder
    var item Named

fn main()
    let b = Bag(tag: \"held\")
    let h = Holder(item: b)
    println(f\"{h.item.label()} {b.label()}\")
",
        ),
        "held held",
    );
}

#[test]
fn test_a_lambda_returning_a_new_value_at_a_trait_type_hands_it_over() {
    assert_heap_guard_output(
        &program(
            "
fn main()
    let mk = fn() Named: Bag(tag: \"made\")
    let n = mk()
    println(n.label())
",
        ),
        "made",
    );
}

#[test]
fn test_a_generic_method_returning_a_new_value_at_a_trait_type_hands_it_over() {
    assert_heap_guard_output(
        "
use system.collections.list

trait Op<T>
    fn get() int

class Impl<V> implements Op<List<V>>
    fn get() int
        return 7

class Mk<V>
    fn make() Op<List<V>>
        return Impl<V>()

fn call(o Op<List<int>>) int
    return o.get()

fn main()
    let m = Mk<String>()
    let o = m.make()
    println(f\"{call(Impl<int>())} {o.get()}\")
",
        "7 7",
    );
}
