// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Assigning a reference-counted value into a field of an existing object.
//!
//! Writing through a field projection hands the reference to the object being
//! written, which releases it when it dies — so the store has to take a
//! reference of its own. Without one the field and the source share a single
//! reference that both release, and the field ends up holding freed memory: the
//! collection reads as empty, or the program crashes, and nothing is reported.
//!
//! Every test reads the field *after* the assignment and asserts the value, and
//! reads the source afterwards too, since the store must not consume it.

use super::utils::*;

#[test]
fn an_array_assigned_into_a_field_keeps_its_elements() {
    assert_runs_with_output(
        r#"
use system.collections.array

struct Holder
    data Array<int, 4>

fn main()
    var src = Array<int, 4>()
    src.set(3, 42)
    var holder = Holder(Array<int, 4>())
    holder.data = src
    println(f"{holder.data.length()} {holder.data[3]}")
"#,
        "4 42",
    );
}

#[test]
fn the_source_of_a_field_assignment_survives_it() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Holder
    data [int]

fn main()
    var src = List([1, 2, 3])
    var holder = Holder(List<int>())
    holder.data = src
    println(f"{holder.data.length()} {src.length()}")
"#,
        "3 3",
    );
}

#[test]
fn a_list_of_strings_assigned_into_a_field_keeps_its_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Holder
    words [String]

fn main()
    var src = List<String>()
    src.push("pe" + "ar")
    var holder = Holder(List<String>())
    holder.words = src
    println(f"{holder.words.length()} {holder.words[0]}")
"#,
        "1 pear",
    );
}

#[test]
fn a_string_assigned_into_a_field_is_readable() {
    assert_runs_with_output(
        r#"
struct Holder
    name String

fn main()
    var holder = Holder("old")
    holder.name = "ne" + "w"
    println(holder.name)
"#,
        "new",
    );
}

/// The second assignment has to release what the first one stored, and no
/// more: a field overwritten twice must end up holding the last value with the
/// first one freed exactly once.
#[test]
fn a_field_assigned_twice_holds_the_last_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Holder
    data [int]

fn main()
    var first = List([1, 2])
    var second = List([7, 8, 9])
    var holder = Holder(List<int>())
    holder.data = first
    holder.data = second
    println(f"{holder.data.length()} {holder.data[2]} {first.length()}")
"#,
        "3 9 2",
    );
}

/// A field holding another struct is the same store one level up: the struct
/// being written is reference-counted like any other managed value.
#[test]
fn a_struct_assigned_into_a_field_keeps_its_own_fields() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Inner
    items [int]

struct Outer
    inner Inner

fn main()
    var replacement = Inner(List([4, 5, 6]))
    var outer = Outer(Inner(List<int>()))
    outer.inner = replacement
    println(f"{outer.inner.items.length()} {outer.inner.items[1]}")
"#,
        "3 5",
    );
}

/// A class field reaches the same store through a different definition lookup,
/// so it is covered beside the struct one.
#[test]
fn a_class_field_assigned_a_collection_keeps_its_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Basket
    public var items [int]

    public fn init(items [int])
        self.items = items

fn main()
    var basket = Basket(List<int>())
    var stock = List([2, 4, 6])
    basket.items = stock
    println(f"{basket.items.length()} {basket.items[2]}")
"#,
        "3 6",
    );
}

/// A write two fields deep. The field holding the inner object stores a pointer
/// to it, so reaching the inner field means loading that pointer rather than
/// adding both offsets to the outer object — a store that adds them lands in
/// the outer object's own slot and leaks what it overwrote.
#[test]
fn a_write_two_fields_deep_lands_in_the_inner_object() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Inner
    tag int
    items [int]

struct Outer
    label int
    inner Inner

fn main()
    var outer = Outer(1, Inner(2, List([0])))
    var fresh = List([7, 8, 9])
    outer.inner.items = fresh
    outer.inner.tag = 5
    println(f"{outer.label} {outer.inner.tag} {outer.inner.items.length()} {outer.inner.items[2]}")
"#,
        "1 5 3 9",
    );
}

/// The same write where every field sits at offset zero: the shape that hides
/// a missing pointer load, because adding zero twice reaches the right address
/// by accident on the read side.
#[test]
fn a_write_two_fields_deep_works_at_offset_zero() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Inner
    items [int]

struct Outer
    inner Inner

fn main()
    var outer = Outer(Inner(List<int>()))
    var fresh = List([4, 5])
    outer.inner.items = fresh
    println(f"{outer.inner.items.length()} {fresh.length()}")
"#,
        "2 2",
    );
}

/// A class one level in reaches the field through a different definition
/// lookup than a struct does, so the depth is covered for both.
#[test]
fn a_write_into_a_class_field_two_levels_deep_lands_correctly() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Cart
    public var items [int]

    public fn init(items [int])
        self.items = items

struct Order
    id int
    cart Cart

fn main()
    var order = Order(7, Cart(List<int>()))
    var restock = List([3, 3, 3])
    order.cart.items = restock
    println(f"{order.id} {order.cart.items.length()} {order.cart.items[0]}")
"#,
        "7 3 3",
    );
}
