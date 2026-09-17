// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Compound assignment writes its arithmetic result through a temp, and that
//! temp is typed from the slot being written. A field's slot is stated by the
//! declaring type, not by the expression that named the field — typed from the
//! latter, a `float` field's `+=` lands in an integer temp and truncates.

use super::utils::*;

#[test]
fn test_float_field_add_assign_keeps_the_fraction() {
    assert_runs_with_output(
        r#"

class Acc
    total float

    fn new()
        self.total = 0.0

    fn add(d float) float
        self.total += d
        return self.total

fn main()
    let a = Acc()
    println(f"{a.add(1.5)}")
    println(f"{a.add(2.25)}")
    "#,
        "1.5\n3.75",
    );
}

#[test]
fn test_float_field_every_compound_operator_keeps_the_fraction() {
    assert_runs_with_output(
        r#"

class Cell
    v float

    fn new(v float)
        self.v = v

    fn apply(d float) float
        self.v *= d
        self.v -= d
        self.v /= d
        return self.v

fn main()
    let c = Cell(4.5)
    println(f"{c.apply(2.0)}")
    "#,
        "3.5",
    );
}

#[test]
fn test_int_field_add_assign_is_unchanged() {
    assert_runs_with_output(
        r#"

class Counter
    n int

    fn new()
        self.n = 0

    fn bump(d int) int
        self.n += d
        return self.n

fn main()
    let c = Counter()
    println(f"{c.bump(3)}")
    println(f"{c.bump(4)}")
    "#,
        "3\n7",
    );
}

#[test]
fn test_float_field_compound_assignment_on_a_struct() {
    assert_runs_with_output(
        r#"

struct Point
    x float
    y float

fn main()
    var p = Point(1.5, 2.0)
    p.x += 2.25
    p.y *= 1.5
    println(f"{p.x}")
    println(f"{p.y}")
    "#,
        "3.75\n3.0",
    );
}

#[test]
fn test_float_field_compound_assignment_on_an_inherited_field() {
    assert_runs_with_output(
        r#"

class Base
    scale float

    fn new(scale float)
        self.scale = scale

class Derived extends Base
    fn grow(d float) float
        self.scale += d
        return self.scale

fn main()
    let d = Derived(1.5)
    println(f"{d.grow(2.25)}")
    "#,
        "3.75",
    );
}
