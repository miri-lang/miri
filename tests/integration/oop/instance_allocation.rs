// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An instance is given memory for the fields it declares.
//!
//! A constructor that calls `init` hands the aggregate a placeholder for every
//! field, because the real values are assigned inside `init`. A placeholder
//! says nothing about how wide its field is — a `float` and an inherited field
//! still spelled in an ancestor's type parameter both arrive as one byte. An
//! instance sized from those widths is smaller than the offsets its own fields
//! are written at, so the constructor writes past the end of the allocation and
//! into whatever the allocator handed out next.
//!
//! Each test reads back the fields that sit beyond such a short allocation, so
//! a wrong value and a crash both fail. Distinct values are essential: fields
//! that all hold the same number cannot show a store landing in the wrong slot.

use super::utils::*;

#[test]
fn every_field_of_a_class_of_floats_reads_back() {
    // Eight pointer-wide fields against eight one-byte placeholders is a gap
    // wide enough that the write past the end leaves the allocation entirely.
    assert_heap_guard_output(
        r#"
class Row
    a float
    b float
    c float
    d float
    e float
    f float
    g float
    h float

    fn init(a float, b float, c float, d float, e float, f float, g float, h float)
        self.a = a
        self.b = b
        self.c = c
        self.d = d
        self.e = e
        self.f = f
        self.g = g
        self.h = h

fn main()
    let r = Row(1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5)
    println(f"{r.a} {r.d} {r.h}")
"#,
        "1.5 4.5 8.5",
    );
}

#[test]
fn a_float_field_left_to_init_does_not_shorten_the_instance() {
    // The smallest form of the same gap: two fields, two placeholders, and a
    // second field written eight bytes into an allocation sized for two.
    assert_heap_guard_output(
        r#"
class Point
    x float
    y float

    fn init(x float, y float)
        self.x = x
        self.y = y

fn main()
    let p = Point(1.5, 2.5)
    println(f"{p.x},{p.y}")
"#,
        "1.5,2.5",
    );
}

#[test]
fn a_managed_field_beside_a_float_survives_the_instance() {
    // A short allocation is not only a wrong value: the reference-counted
    // field's pointer is what lands outside it here, and releasing the
    // instance then frees whatever that overlapped.
    assert_heap_guard_output(
        r#"
class Tagged
    weight float
    label String
    count int

    fn init(weight float, label String, count int)
        self.weight = weight
        self.label = label
        self.count = count

fn main()
    let t = Tagged(1.5, "h" + "i", 7)
    println(f"{t.weight} {t.label} {t.count}")
"#,
        "1.5 hi 7",
    );
}

#[test]
fn an_inherited_field_is_counted_in_the_instance_it_belongs_to() {
    // The fields an instance holds are not all declared where the instance's
    // own class is. A child sized by what it declares itself leaves out
    // everything the ancestor contributes, and `super.init` writes those into
    // memory the instance was never given.
    assert_heap_guard_output(
        r#"
class Base
    a float
    b float

    fn init(a float, b float)
        self.a = a
        self.b = b

class Child extends Base
    c float
    d float

    fn init(a float, b float, c float, d float)
        super.init(a, b)
        self.c = c
        self.d = d

fn main()
    let x = Child(1.5, 2.5, 3.5, 4.5)
    println(f"{x.a} {x.b} {x.c} {x.d}")
"#,
        "1.5 2.5 3.5 4.5",
    );
}

#[test]
fn an_inherited_generic_field_is_counted_at_the_width_it_is_read_at() {
    // An inherited field is written in the ancestor's type parameters, which
    // name nothing in the child, so its placeholder cannot take a width from
    // them at all. The instance still has to be large enough for the slot the
    // field is read back from.
    assert_heap_guard_output(
        r#"
class Base<A, B>
    left A
    right B

    fn init(left A, right B)
        self.left = left
        self.right = right

class Child<X, Y> extends Base<X, Y>
    fn init(left X, right Y)
        super.init(left, right)

fn main()
    let c = Child<float, int>(1.5, 2)
    println(f"{c.left},{c.right}")
"#,
        "1.5,2",
    );
}

#[test]
fn a_class_with_no_init_places_a_default_where_its_field_is_read() {
    // Here the aggregate's own stores are the field initialization, so the
    // offsets it writes at have to be the offsets a field read comes back to.
    assert_heap_guard_output(
        r#"
class Point
    x float
    y float

fn main()
    var p = Point()
    p.x = 1.5
    p.y = 2.5
    println(f"{p.x},{p.y}")
"#,
        "1.5,2.5",
    );
}
