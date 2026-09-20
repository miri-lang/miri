// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Unary minus and numeric casts written against a generic parameter.
//!
//! A generic body is checked once, against its own parameters, so an operator
//! applied to a value of a parameter type has no type to ask yet. Binary
//! arithmetic is admitted there and answered where the parameter is pinned;
//! these tests hold unary minus and a numeric cast to the same bargain, and
//! pin the two halves that bargain needs to be worth anything:
//!
//!   * every instantiation computes the **value** the operator means, at each
//!     width — a result temp typed at the bare parameter instead of the type
//!     the instantiation supplies truncates silently, and
//!   * an instantiation that pins the parameter to a type the operator has no
//!     meaning for is refused, naming the body and the parameter, rather than
//!     reaching code generation with nothing but the operand's bytes.

use super::utils::{assert_compiler_error, assert_runs_with_output};

#[test]
fn negating_a_generic_parameter_instantiated_at_float_keeps_the_fraction() {
    assert_runs_with_output(
        r#"
fn neg<T>(a T) T
    return -a

fn main()
    println(f"{neg(1.5)}")
"#,
        "-1.5",
    );
}

/// The narrower float width. A result temp typed at the bare parameter falls
/// back to the pointer-width integer, which truncates the value on its way in.
#[test]
fn negating_a_generic_parameter_instantiated_at_f32_keeps_the_fraction() {
    assert_runs_with_output(
        r#"
fn neg<T>(a T) T
    return -a

fn main()
    let x f32 = 2.25
    println(f"{neg(x)}")
"#,
        "-2.25",
    );
}

#[test]
fn negating_a_generic_parameter_instantiated_at_int_answers_the_integer() {
    assert_runs_with_output(
        r#"
fn neg<T>(a T) T
    return -a

fn main()
    println(f"{neg(7)}")
"#,
        "-7",
    );
}

/// One body reached at two widths in the same program: each monomorphized copy
/// has to carry its own instantiation's type, not whichever was lowered last.
#[test]
fn one_negating_body_instantiated_at_two_widths_answers_both() {
    assert_runs_with_output(
        r#"
fn neg<T>(a T) T
    return -a

fn main()
    println(f"{neg(1.5)}")
    println(f"{neg(7)}")
"#,
        "-1.5\n-7",
    );
}

/// Unary minus reached through a field of a generic class rather than a
/// parameter of a generic function: the projection has to keep the field's
/// type, which the instantiation supplies.
#[test]
fn negating_a_generic_class_field_instantiated_at_float_keeps_the_fraction() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn negated() T
        return -self.v

fn main()
    let b = Box<float>(2.5)
    println(f"{b.negated()}")
"#,
        "-2.5",
    );
}

/// Unary plus is the identity, and must stay the identity at a parameter: the
/// earlier attempt at this feature had it answering `0.0`.
#[test]
fn unary_plus_on_a_generic_parameter_answers_the_operand() {
    assert_runs_with_output(
        r#"
fn same<T>(a T) T
    return +a

fn main()
    println(f"{same(1.5)}")
"#,
        "1.5",
    );
}

#[test]
fn casting_a_generic_parameter_to_float_answers_the_widened_value() {
    assert_runs_with_output(
        r#"
fn twice_as_float<T>(a T) float
    return (a + a) as float

fn main()
    println(f"{twice_as_float(3)}")
"#,
        "6.0",
    );
}

/// The cast reads its source through the instantiation too: a body pinned at
/// `float` casting down to `int` must truncate the instantiation's value, not
/// a value re-read at the bare parameter.
#[test]
fn casting_a_float_instantiated_parameter_to_int_truncates_the_value() {
    assert_runs_with_output(
        r#"
fn to_int<T>(a T) int
    return a as int

fn main()
    println(f"{to_int(2.75)}")
"#,
        "2",
    );
}

/// The other half of the bargain. A class with no arithmetic pinned into a
/// body that negates its parameter is refused where the type is chosen — the
/// body itself cannot know, and code generation would have only the bytes.
#[test]
fn negating_a_parameter_pinned_to_a_class_is_refused_at_the_instantiation() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn neg<T>(a T) T
    return -a

fn main()
    let p = neg(Plain(1))
    println(f"{p.n}")
"#,
        "'neg' applies '-' to its 'T' parameter",
    );
}

#[test]
fn negating_a_parameter_pinned_to_a_string_is_refused_at_the_instantiation() {
    assert_compiler_error(
        r#"
fn neg<T>(a T) T
    return -a

fn main()
    println(f"{neg('x')}")
"#,
        "'neg' applies '-' to its 'T' parameter",
    );
}

#[test]
fn negating_a_parameter_pinned_to_a_bool_is_refused_at_the_instantiation() {
    assert_compiler_error(
        r#"
fn neg<T>(a T) T
    return -a

fn main()
    println(f"{neg(true)}")
"#,
        "'neg' applies '-' to its 'T' parameter",
    );
}

#[test]
fn casting_a_parameter_pinned_to_a_string_is_refused_at_the_instantiation() {
    assert_compiler_error(
        r#"
fn to_float<T>(a T) float
    return a as float

fn main()
    println(f"{to_float('x')}")
"#,
        "'to_float' casts its 'T' parameter",
    );
}

/// A body that negates a parameter it was handed by another generic body states
/// nothing new: the requirement is handed on to whoever pins the outer body, so
/// the refusal still lands at the site that chose the type.
#[test]
fn a_negation_requirement_handed_through_a_delegating_body_is_answered_at_the_outer_site() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn neg<T>(a T) T
    return -a

fn neg_twice<U>(a U) U
    return neg(neg(a))

fn main()
    let p = neg_twice(Plain(1))
    println(f"{p.n}")
"#,
        "parameter",
    );
}

/// A body that never negates its parameter constrains none of its
/// instantiations — the requirement is stated by the operator, not by being
/// generic.
#[test]
fn a_body_that_does_not_negate_its_parameter_admits_a_class_instantiation() {
    assert_runs_with_output(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn pass_through<T>(a T) T
    return a

fn main()
    let p = pass_through(Plain(4))
    println(f"{p.n}")
"#,
        "4",
    );
}

/// A method reached through a generic receiver records its site the same way a
/// call to a generic function does, so negating a class parameter is refused
/// where the receiver's type argument is chosen.
#[test]
fn negating_a_generic_class_parameter_is_refused_at_the_receiver_instantiation() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn negated() T
        return -self.v

fn main()
    let b = Box<Plain>(Plain(1))
    println(f"{b.negated().n}")
"#,
        "'negated' applies '-' to its 'T' parameter",
    );
}
