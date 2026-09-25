// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Arithmetic written against a generic parameter, in a body instantiated at a
//! concrete type. The value the operator produces is held in a temp, and that
//! temp must be typed at the instantiation rather than at the parameter — a
//! temp left at the parameter is pointer-width integer, which truncates a float
//! result and makes the backend reject a second operation reading it.

use super::utils::*;

#[test]
fn test_generic_addition_through_a_lambda_at_float() {
    assert_runs_with_output(
        r#"

fn sum2<T>(a T, b T) T
    let f = fn(x T, y T) T: x + y
    return f(a, b)

fn main()
    println(f"{sum2(1.5, 2.25)}")
    "#,
        "3.75",
    );
}

#[test]
fn test_generic_nested_addition_at_float() {
    assert_runs_with_output(
        r#"

fn sum3<T>(a T, b T) T
    return (a + b) + a

fn main()
    println(f"{sum3(1.5, 2.25)}")
    "#,
        "5.25",
    );
}

#[test]
fn test_generic_nested_addition_at_f32() {
    assert_runs_with_output(
        r#"

fn sum3<T>(a T, b T) T
    return (a + b) + a

fn main()
    let a f32 = 1.5
    let b f32 = 2.25
    println(f"{sum3(a, b)}")
    "#,
        "5.25",
    );
}

#[test]
fn test_generic_nested_addition_at_int_is_unchanged() {
    assert_runs_with_output(
        r#"

fn sum3<T>(a T, b T) T
    return (a + b) + a

fn main()
    println(f"{sum3(3, 4)}")
    "#,
        "10",
    );
}

#[test]
fn test_generic_mixed_arithmetic_at_float() {
    assert_runs_with_output(
        r#"

fn blend<T>(a T, b T) T
    return ((a * b) - a) / b

fn main()
    println(f"blend={blend(4.0, 2.0)} end")
    "#,
        "blend=2.0 end",
    );
}

#[test]
fn test_generic_comparison_at_float_still_orders() {
    assert_runs_with_output(
        r#"

fn smaller<T>(a T, b T) bool
    return a < b

fn main()
    println(f"{smaller(1.5, 2.25)}")
    println(f"{smaller(2.25, 1.5)}")
    "#,
        "true\nfalse",
    );
}

#[test]
fn test_generic_comparison_of_a_computed_sum_at_float() {
    assert_runs_with_output(
        r#"

fn sum_exceeds<T>(a T, b T) bool
    return (a + b) > a

fn main()
    println(f"{sum_exceeds(1.5, 2.25)}")
    println(f"{sum_exceeds(1.5, 0.0 - 2.25)}")
    "#,
        "true\nfalse",
    );
}

#[test]
fn test_generic_field_compound_assignment_at_float() {
    assert_runs_with_output(
        r#"

class Box<T>
    v T

    fn new(v T)
        self.v = v

    fn bump(d T) T
        self.v += d
        return self.v

fn main()
    let b = Box<float>(1.5)
    println(f"{b.bump(2.25)}")
    "#,
        "3.75",
    );
}

#[test]
fn test_generic_compound_assignment_at_float() {
    assert_runs_with_output(
        r#"

fn accumulate<T>(a T, b T) T
    var total = a
    total += b
    total += a
    return total

fn main()
    println(f"{accumulate(1.5, 2.25)}")
    "#,
        "5.25",
    );
}

#[test]
fn test_generic_arithmetic_in_a_class_method_at_float() {
    assert_runs_with_output(
        r#"

class Pair<T>
    first T
    second T

    fn new(first T, second T)
        self.first = first
        self.second = second

    fn combined() T
        return (self.first + self.second) + self.first

fn main()
    let p = Pair<float>(1.5, 2.25)
    println(f"{p.combined()}")
    "#,
        "5.25",
    );
}

/// Arithmetic on an unbounded type parameter is admitted in the body: the
/// operand has no type to ask yet. The body records what it applies to the
/// parameter, and every site that pins the parameter to a concrete type answers
/// for it, so a type with no arithmetic is refused there rather than reaching
/// code generation.
#[test]
fn arithmetic_on_a_generic_parameter_is_refused_at_a_class_with_no_operator() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn add<T>(a T, b T) T
    return a + b

fn main()
    let s = add(Plain(1), Plain(2))
    println(f"{s.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

/// Without the check the operands are or-ed as bits, which answers `true` and
/// is not addition.
#[test]
fn arithmetic_on_a_generic_parameter_is_refused_at_a_boolean() {
    assert_compiler_error(
        r#"
fn add<T>(a T, b T) T
    return a + b

fn main()
    let s = add(true, false)
    println(f"{s}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_generic_addition_at_string_concatenates() {
    assert_runs_with_output(
        r#"

fn add<T>(a T, b T) T
    return a + b

fn main()
    let joined = add("x", "y")
    println(f"{joined}")
    "#,
        "xy",
    );
}

#[test]
fn test_generic_addition_at_a_class_that_adds() {
    assert_runs_with_output(
        r#"
use system.ops

class Money implements Addable
    n int

    fn init(n int)
        self.n = n

    public fn concat(other Self) Self
        return Money(self.n + other.n)

fn add<T>(a T, b T) T
    return a + b

fn main()
    let total = add(Money(2), Money(3))
    println(f"{total.n}")
    "#,
        "5",
    );
}

#[test]
fn test_sum_over_strings_concatenates_and_over_numbers_adds() {
    assert_runs_with_output(
        r#"

fn main()
    let words = ["a", "b", "c"]
    let joined = words.sum() ?? "none"
    println(f"{joined}")
    let numbers = [1, 2, 3]
    let total = numbers.sum() ?? 0
    println(f"{total}")
    "#,
        "abc\n6",
    );
}

#[test]
fn subtraction_on_a_generic_parameter_is_refused_at_a_string() {
    assert_compiler_error(
        r#"
fn take<T>(a T, b T) T
    return a - b

fn main()
    let s = take("x", "y")
    println(f"{s}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn multiplication_on_a_generic_parameter_is_refused_at_two_strings() {
    assert_compiler_error(
        r#"
fn scale<T>(a T, b T) T
    return a * b

fn main()
    let s = scale("x", "y")
    println(f"{s}")
"#,
        "cannot multiply String by String",
    );
}

#[test]
fn subtraction_on_a_generic_parameter_is_refused_at_a_class_that_only_adds() {
    assert_compiler_error(
        r#"
use system.ops

class Money implements Addable
    n int

    fn init(n int)
        self.n = n

    public fn concat(other Self) Self
        return Money(self.n + other.n)

fn take<T>(a T, b T) T
    return a - b

fn main()
    let m = take(Money(3), Money(1))
    println(f"{m.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_mixing_a_generic_parameter_with_another_type_is_refused() {
    assert_compiler_error(
        r#"
fn mark<T>(a T) T
    return a + 'x'

fn main()
    let s = mark(3)
    println(f"{s}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_a_body_delegates_is_refused_where_the_outer_body_is_pinned() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn add<T>(a T, b T) T
    return a + b

fn outer<T>(a T, b T) T
    return add(a, b)

fn main()
    let s = outer(Plain(1), Plain(2))
    println(f"{s.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_in_a_body_that_pins_its_own_parameter_is_refused() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn add<T>(a T, b T, again bool) T
    if again
        return add(a, b, false)
    return a + b

fn main()
    let s = add(Plain(1), Plain(2), true)
    println(f"{s.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_on_a_generic_field_is_refused_where_the_class_is_pinned() {
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

    fn doubled() T
        return self.v + self.v

fn main()
    let b = Box<Plain>(Plain(1))
    let d = b.doubled()
    println(f"{d.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_in_a_default_method_is_refused_where_the_trait_is_pinned() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

trait Doubling<T>
    abstract fn value() T

    public fn doubled() T
        let v = self.value()
        return v + v

class Holder<T> implements Doubling<T>
    p T

    fn init(p T)
        self.p = p

    public fn value() T
        return self.p

fn main()
    let h = Holder<Plain>(Plain(1))
    let d = h.doubled()
    println(f"{d.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn summing_a_list_of_a_class_that_does_not_add_is_refused() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn main()
    let items = [Plain(1), Plain(2)]
    let total = items.sum()
    println("done")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn refusing_arithmetic_at_an_instantiation_names_the_body_and_the_parameter() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn add<T>(a T, b T) T
    return a + b

fn main()
    let s = add(Plain(1), Plain(2))
    println(f"{s.n}")
"#,
        "'add' applies '+' to its 'T' parameter",
    );
}

/// `x += b` applies `+` exactly as `x = x + b` does, so it states the same
/// requirement on `T`. Without it the body is accepted at a struct and the
/// backend adds two pointers.
#[test]
fn compound_addition_on_a_generic_parameter_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

fn accumulate<T>(a T, b T) T
    var x T = a
    x += b
    return x

fn main()
    let p = accumulate(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn compound_addition_on_a_generic_field_is_refused_at_a_class_with_no_operator() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

class Holder<T>
    public v T
    fn init(v T)
        self.v = v

fn grow<T>(a T, b T) T
    var h = Holder<T>(v: a)
    h.v += b
    return h.v

fn main()
    let p = grow(Plain(1), Plain(2))
    println(f"{p.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}
