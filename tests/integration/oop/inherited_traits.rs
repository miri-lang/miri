// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A trait a class implements is implemented by every class that extends it.
//!
//! A trait method taking `Self` as a parameter (`compare`, `equals`) is
//! answered by the body the subclass inherits: that body accepts any instance
//! of the base, which a subclass instance is. A trait method returning `Self`
//! (`clone`) is not: the inherited body builds the base, so each concrete
//! subclass declares its own.

use super::utils::*;

const COMPARABLE_MEASURE: &str = r#"
use system.collections.list

class Measure implements Comparable
    value int

    fn init(value int)
        self.value = value

    public fn compare(other Self) int
        return self.value - other.value
"#;

const EQUATABLE_MEASURE: &str = r#"
use system.ops

class Measure implements Equatable
    value int

    fn init(value int)
        self.value = value

    public fn equals(other Self) bool
        return self.value % 10 == other.value % 10
"#;

const CLONEABLE_MEASURE: &str = r#"
use system.collections.list

class Measure implements Cloneable
    value int

    fn init(value int)
        self.value = value

    public fn clone() Self
        return Measure(self.value)
"#;

fn program(base: &str, rest: &str) -> String {
    format!("{base}\n{rest}")
}

#[test]
fn test_subclass_orders_through_the_compare_its_base_implements() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Length extends Measure
    fn init(value int)
        super.init(value)

fn main()
    let a = Length(1)
    let b = Length(2)
    println(f"{a < b} {a <= b} {a > b} {a >= b}")
"#,
        ),
        "true true false false",
    );
}

#[test]
fn test_subclass_restating_comparable_orders_through_the_inherited_compare() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Length extends Measure implements Comparable
    fn init(value int)
        super.init(value)

fn main()
    let a = Length(5)
    let b = Length(2)
    println(f"{a < b} {a > b}")
"#,
        ),
        "false true",
    );
}

#[test]
fn test_grandchild_orders_through_a_compare_two_classes_up() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Length extends Measure
    fn init(value int)
        super.init(value)

class Span extends Length
    fn init(value int)
        super.init(value)

fn main()
    let a = Span(7)
    let b = Span(3)
    println(f"{a < b} {a > b}")
"#,
        ),
        "false true",
    );
}

#[test]
fn test_subclass_overriding_compare_orders_through_its_own_body() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Reversed extends Measure
    fn init(value int)
        super.init(value)

    public fn compare(other Measure) int
        return other.value - self.value

fn main()
    let a = Reversed(1)
    let b = Reversed(2)
    println(f"{a < b} {a > b}")
"#,
        ),
        "false true",
    );
}

#[test]
fn test_sorting_a_list_of_subclass_uses_the_inherited_compare() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Length extends Measure
    fn init(value int)
        super.init(value)

fn main()
    var l = List<Length>()
    l.push(Length(20))
    l.push(Length(10))
    l.push(Length(30))
    l.sort()
    println(f"{l[0].value},{l[1].value},{l[2].value}")
"#,
        ),
        "10,20,30",
    );
}

#[test]
fn test_sorting_a_list_of_subclass_uses_its_overriding_compare() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Reversed extends Measure
    fn init(value int)
        super.init(value)

    public fn compare(other Measure) int
        return other.value - self.value

fn main()
    var l = List<Reversed>()
    l.push(Reversed(20))
    l.push(Reversed(10))
    l.push(Reversed(30))
    l.sort()
    println(f"{l[0].value},{l[1].value},{l[2].value}")
"#,
        ),
        "30,20,10",
    );
}

#[test]
fn test_subclass_compares_equal_through_the_equals_its_base_implements() {
    // `equals` looks only at the last digit, so a structural comparison of
    // the fields would answer differently for 3 and 13.
    assert_runs_with_output(
        &program(
            EQUATABLE_MEASURE,
            r#"
class Length extends Measure
    fn init(value int)
        super.init(value)

fn main()
    let a = Length(3)
    let b = Length(13)
    let c = Length(4)
    println(f"{a == b} {a != b} {a == c}")
"#,
        ),
        "true false false",
    );
}

#[test]
fn test_subclass_restating_equatable_compares_through_the_inherited_equals() {
    assert_runs_with_output(
        &program(
            EQUATABLE_MEASURE,
            r#"
class Length extends Measure implements Equatable
    fn init(value int)
        super.init(value)

fn main()
    let a = Length(3)
    let b = Length(13)
    println(f"{a == b}")
"#,
        ),
        "true",
    );
}

#[test]
fn test_subclass_declaring_its_own_clone_copies_its_own_fields() {
    assert_runs_with_output(
        &program(
            CLONEABLE_MEASURE,
            r#"
class Length extends Measure
    unit String

    fn init(value int, unit String)
        super.init(value)
        self.unit = unit

    public fn clone() Self
        return Length(self.value, self.unit)

fn main()
    var l = List<Length>()
    l.push(Length(1, "cm"))
    let copies = l.clone()
    let single = Length(2, "mm").clone()
    println(f"{copies[0].value}{copies[0].unit} {single.value}{single.unit}")
"#,
        ),
        "1cm 2mm",
    );
}

#[test]
fn test_subclass_inheriting_clone_without_its_own_is_refused() {
    assert_compiler_error(
        &program(
            CLONEABLE_MEASURE,
            r#"
class Length extends Measure
    unit String

    fn init(value int, unit String)
        super.init(value)
        self.unit = unit

fn main()
    var l = List<Length>()
    l.push(Length(1, "cm"))
    let copies = l.clone()
    println(copies[0].unit)
"#,
        ),
        "Class 'Length' must declare its own 'clone'",
    );
}

#[test]
fn test_subclass_restating_cloneable_without_its_own_clone_is_refused() {
    assert_compiler_error(
        &program(
            CLONEABLE_MEASURE,
            r#"
class Length extends Measure implements Cloneable
    fn init(value int)
        super.init(value)

fn main()
    println(f"{Length(1).value}")
"#,
        ),
        "Class 'Length' must declare its own 'clone'",
    );
}

#[test]
fn test_abstract_subclass_declaring_clone_abstract_leaves_it_to_its_descendants() {
    assert_runs_with_output(
        &program(
            CLONEABLE_MEASURE,
            r#"
abstract class Scaled extends Measure
    fn init(value int)
        super.init(value)

    public abstract fn clone() Self

class Length extends Scaled
    fn init(value int)
        super.init(value)

    public fn clone() Self
        return Length(self.value + 100)

fn main()
    var l = List<Length>()
    l.push(Length(1))
    let copies = l.clone()
    println(f"{Length(1).clone().value} {copies[0].value}")
"#,
        ),
        "101 101",
    );
}

#[test]
fn test_abstract_subclass_inheriting_clone_without_declaring_it_is_refused() {
    assert_compiler_error(
        &program(
            CLONEABLE_MEASURE,
            r#"
abstract class Scaled extends Measure
    fn init(value int)
        super.init(value)

class Length extends Scaled
    fn init(value int)
        super.init(value)

    public fn clone() Self
        return Length(self.value)

fn main()
    println(f"{Length(1).value}")
"#,
        ),
        "Class 'Scaled' must declare its own 'clone'",
    );
}

#[test]
fn test_concrete_descendant_of_an_abstract_clone_must_implement_it() {
    assert_compiler_error(
        &program(
            CLONEABLE_MEASURE,
            r#"
abstract class Scaled extends Measure
    fn init(value int)
        super.init(value)

    public abstract fn clone() Self

class Length extends Scaled
    fn init(value int)
        super.init(value)

fn main()
    println(f"{Length(1).value}")
"#,
        ),
        "Class 'Length' must implement abstract method 'clone'",
    );
}

#[test]
fn test_subclass_restating_comparable_with_an_override_over_the_base_type_orders_by_it() {
    assert_runs_with_output(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Reversed extends Measure implements Comparable
    fn init(value int)
        super.init(value)

    public fn compare(other Measure) int
        return other.value - self.value

fn main()
    println(f"{Reversed(1) < Reversed(2)}")
"#,
        ),
        "false",
    );
}

#[test]
fn test_override_narrowing_a_self_parameter_to_the_subclass_is_refused() {
    // A call through the base type may hand the override any `Measure`, so an
    // override typed over the subclass would read fields a `Measure` lacks.
    assert_compiler_error(
        &program(
            COMPARABLE_MEASURE,
            r#"
class Reversed extends Measure
    fn init(value int)
        super.init(value)

    public fn compare(other Self) int
        return other.value - self.value

fn main()
    println(f"{Reversed(1) < Reversed(2)}")
"#,
        ),
        "Method 'compare' has incompatible parameter type for 'other'",
    );
}

#[test]
fn test_set_of_subclass_matches_elements_through_the_inherited_equals() {
    assert_runs_with_output(
        &program(
            EQUATABLE_MEASURE,
            r#"
use system.collections.set

class Length extends Measure
    fn init(value int)
        super.init(value)

fn main()
    var s = Set<Length>()
    s.add(Length(3))
    s.add(Length(13))
    println(f"{s.length()} {s.contains(Length(23))}")
"#,
        ),
        "1 true",
    );
}

const ADDABLE_MONEY: &str = r#"
use system.ops

class Money implements Addable
    cents int

    fn init(cents int)
        self.cents = cents

    public fn concat(other Self) Self
        return Money(self.cents + other.cents)
"#;

#[test]
fn test_subclass_inheriting_concat_without_its_own_is_refused() {
    assert_compiler_error(
        &program(
            ADDABLE_MONEY,
            r#"
class Euro extends Money
    fn init(cents int)
        super.init(cents)

fn main()
    println(f"{(Euro(1) + Euro(2)).cents}")
"#,
        ),
        "Class 'Euro' must declare its own 'concat'",
    );
}

#[test]
fn test_subclass_override_returning_itself_adds_through_its_own_concat() {
    // The override accepts every `Money` its base does and narrows only the
    // return, so `+` on two `Euro` values yields a `Euro` with its own field.
    assert_runs_with_output(
        &program(
            ADDABLE_MONEY,
            r#"
class Euro extends Money
    tag String

    fn init(cents int, tag String)
        super.init(cents)
        self.tag = tag

    public fn concat(other Money) Self
        return Euro(self.cents + other.cents, self.tag)

fn main()
    let e = Euro(1, "eur") + Euro(2, "x")
    println(f"{e.cents} {e.tag}")
"#,
        ),
        "3 eur",
    );
}
