// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Two type parameters a body declares are two types. Each is bound by its own
//! caller, so nothing the body can see makes a `T` an `U`: accepting one where
//! the other is expected lets a generic body reinterpret whatever it is handed
//! — a string read back as an integer, an integer released as a string.
//!
//! A parameter is still every type its `extends` bound names, and a name no
//! body declares is an inference slot the call fills in, so both stay open.

use super::utils::*;

#[test]
fn a_parameter_is_not_returned_as_another() {
    assert_compiler_error(
        r#"
fn cast<T, U>(a T) U
    return a

fn main()
    let x int = cast<String, int>("a" + "b")
    println(f"{x}")
"#,
        "expected U, got T",
    );
}

#[test]
fn a_parameter_is_not_bound_to_a_local_written_at_another() {
    assert_compiler_error(
        r#"
fn confuse<A, B>(seed A) B
    let x B = seed
    return x

fn main()
    let s String = confuse(5)
    println(s)
"#,
        "expected B, got A",
    );
}

/// Both parameters meet the same bound, so a `U` does everything a `T` is
/// required to — but a caller binds `T` to its own class, which a `U` need not
/// be.
#[test]
fn a_parameter_meeting_anothers_bound_is_not_that_parameter() {
    assert_compiler_error(
        r#"
class Animal
    name String
    fn init(name String)
        self.name = name

fn pick<T extends Animal, U extends Animal>(u U) T
    return u

fn main()
    println(pick<Animal, Animal>(Animal("a" + "b")).name)
"#,
        "expected T, got U",
    );
}

#[test]
fn a_parameter_bounded_by_another_is_returned_as_it() {
    assert_heap_guard_output(
        r#"
class Animal
    name String
    fn init(name String)
        self.name = name

fn widen<T extends Animal, U extends T>(u U) T
    return u

fn main()
    let a = widen<Animal, Animal>(Animal("r" + "ex"))
    println(a.name)
"#,
        "rex",
    );
}

#[test]
fn a_parameter_is_its_own_type() {
    assert_heap_guard_output(
        r#"
fn keep<T>(a T) T
    var x T = a
    let y T = x
    return y

fn main()
    println(keep("a" + "b"))
    println(f"{keep(7)}")
"#,
        "ab\n7",
    );
}

/// The callee names its parameters in the opposite order to the caller. The
/// callee's `T` is filled in by the call, not the caller's `T`.
#[test]
fn a_callees_parameters_are_not_the_callers_of_the_same_name() {
    assert_heap_guard_output(
        r#"
fn h<U, T>(a U, b T) U
    return a

fn g<T>(x T) T
    return h(x, 5)

fn main()
    println(f"{g(7)}")
    println(g("a" + "b"))
"#,
        "7\nab",
    );
}

#[test]
fn a_callee_result_at_its_second_parameter_is_the_callers_second() {
    assert_heap_guard_output(
        r#"
fn h<U, T>(a U, b T) T
    return b

fn g<T, U>(x T, y U) U
    return h(x, y)

fn main()
    println(g(7, "a" + "b"))
"#,
        "ab",
    );
}

/// An operator across two parameters is decided per instantiation, so the
/// body stays open and a caller that binds both to `int` runs.
#[test]
fn an_operator_across_two_parameters_is_decided_per_instantiation() {
    assert_runs_with_output(
        r#"
fn add<T, U>(a T, b U) T
    return a + b

fn main()
    println(f"{add(1, 2)}")
"#,
        "3",
    );
}

/// A caller may bind `T` to anything, so an `int` the body made itself is not
/// a `T` either.
#[test]
fn a_concrete_value_is_not_a_parameter() {
    assert_compiler_error(
        r#"
fn f<T>(a T) T
    let n int = 5
    var x T = n
    return x

fn main()
    println(f("a" + "b"))
"#,
        "expected T, got int",
    );
}

/// A `Dog` meets `T`'s bound, but a caller may bind `T` to another subclass.
#[test]
fn a_subclass_is_not_a_parameter_bounded_by_its_parent() {
    assert_compiler_error(
        r#"
class Animal
    fn init()
        let _z = 0

class Dog extends Animal
    fn init()
        super.init()

fn f<T extends Animal>(d Dog) T
    return d

fn main()
    let _a = f<Animal>(Dog())
"#,
        "expected T, got Dog",
    );
}

/// A function type a body writes in its own parameter is the same type the
/// callee writes in its: `fn(T) T` names the caller's `T` on both sides once
/// the call binds the callee's.
#[test]
fn a_function_typed_parameter_is_handed_to_a_generic_callee() {
    assert_runs_with_output(
        r#"
fn apply<T>(x T, f fn(T) T) T
    return f(x)

fn twice<T>(x T, g fn(T) T) T
    return apply(x, g)

fn main()
    println(f"{twice(5, fn(y int) int: y + 1)}")
"#,
        "6",
    );
}

#[test]
fn a_lambda_written_at_the_callers_parameter_is_handed_to_a_generic_callee() {
    assert_runs_with_output(
        r#"
fn apply<T>(x T, f fn(T) T) T
    return f(x)

fn twice<S>(x S) S
    let g = fn(y S) S: y
    return apply(x, g)

fn main()
    println(f"{twice(5)}")
"#,
        "5",
    );
}

#[test]
fn an_inline_lambda_at_the_callers_parameter_is_handed_to_a_generic_callee() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn keep<T>(xs [T], f fn(T) bool) int
    var n = 0
    for x in xs
        if f(x)
            n = n + 1
    return n

fn count_all<T>(xs [T]) int
    return keep(xs, fn(v T) bool: true)

fn main()
    println(f"{count_all(List([1, 2, 3]))}")
"#,
        "3",
    );
}

#[test]
fn a_function_returning_an_optional_parameter_is_handed_to_a_generic_callee() {
    assert_runs_with_output(
        r#"
fn apply2<T>(x T, f fn(T) T?) T?
    return f(x)

fn go<T>(x T, g fn(T) T?) T?
    return apply2(x, g)

fn main()
    let r = go(3, fn(y int) int?: y + 1)
    println(f"{r ?? 0}")
"#,
        "4",
    );
}

/// The callee's `U extends T` is the callee's own: a caller whose `V` extends
/// its `U` has not said that `V` is its `T`, though both names are in scope
/// while the call is checked.
#[test]
fn a_callees_bound_does_not_stand_for_the_callers() {
    assert_compiler_error(
        r#"
fn h<T, U extends T>(a T, b T, c U) T
    return b

fn g<T, U, V extends U>(t T, u U, v V) T
    return h(t, v, t)

fn main()
    let r = g(7, "a" + "b", "c" + "d")
    println(f"{r}")
"#,
        "expected T, got V",
    );
}

#[test]
fn a_callees_bound_does_not_stand_for_the_callers_under_another_name() {
    assert_compiler_error(
        r#"
fn h<T, U extends T>(a T, b T, c U) T
    return b

fn g<T, X, V extends X>(t T, u X, v V) T
    return h(t, v, t)

fn main()
    let r = g(7, "a" + "b", "c" + "d")
    println(f"{r}")
"#,
        "expected T, got V",
    );
}

#[test]
fn a_chain_of_bounds_in_one_declaration_reaches_its_end() {
    assert_heap_guard_output(
        r#"
class Animal
    name String
    fn init(name String)
        self.name = name

fn widest<T extends Animal, U extends T, V extends U>(v V) T
    return v

fn main()
    println(widest<Animal, Animal, Animal>(Animal("r" + "ex")).name)
"#,
        "rex",
    );
}

/// The operand beside a parameter is judged with it where the call binds the
/// parameter.
#[test]
fn arithmetic_between_a_parameter_and_a_literal_runs() {
    assert_runs_with_output(
        r#"
fn inc<T>(a T) T
    return a + 1

fn scale<T>(a T, k int) T
    return a * k

fn main()
    println(f"{inc(3)} {scale(3, 2)}")
"#,
        "4 6",
    );
}

#[test]
fn arithmetic_between_a_parameter_and_a_literal_is_refused_where_it_is_bound_to_text() {
    assert_compiler_error(
        r#"
fn inc<T>(a T) T
    return a + 1

fn main()
    println(inc("a" + "b"))
"#,
        "'inc' applies '+' to its 'T' parameter",
    );
}

#[test]
fn a_parameter_compared_with_a_literal_runs() {
    assert_runs_with_output(
        r#"
fn lt<T>(a T) bool
    return a < 10

fn is_zero<T>(a T) bool
    return a == 0

fn main()
    println(f"{lt(3)} {lt(30)} {is_zero(0)} {is_zero(3)}")
"#,
        "true false true false",
    );
}

#[test]
fn a_parameter_ordered_against_a_literal_is_refused_where_it_is_bound_to_text() {
    assert_compiler_error(
        r#"
fn lt<T>(a T) bool
    return a < 10

fn main()
    let r = lt("a" + "b")
    println(f"{r}")
"#,
        "'lt' applies '<' to its 'T' parameter",
    );
}

#[test]
fn a_parameter_equated_with_a_literal_is_refused_where_it_is_bound_to_text() {
    assert_compiler_error(
        r#"
fn is_zero<T>(a T) bool
    return a == 0

fn main()
    let r = is_zero("a" + "b")
    println(f"{r}")
"#,
        "'is_zero' applies '==' to its 'T' parameter",
    );
}

/// A method's own `T` would be read as its class's, so it is refused where it
/// is declared rather than letting the call below judge a string against the
/// class's `int`.
#[test]
fn a_method_parameter_shadowing_the_class_parameter_lets_no_mismatch_through() {
    assert_compiler_error(
        r#"
class Box<T>
    item T
    fn init(item T)
        self.item = item
    fn swap<T>(x T) T
        return self.item

fn main()
    let b = Box<int>(3)
    println(b.swap("a" + "b"))
"#,
        "Method 'swap' declares a type parameter 'T' its class already declares",
    );
}

/// The caller's `fn(x U) U` is handed on as the callee's `fn(x T) T`, first
/// argument or last: the callee's `T` is bound to the caller's `U`, and the
/// `U` written inside the function type is that same parameter.
#[test]
fn a_function_typed_parameter_binds_the_callees_parameter_in_either_position() {
    assert_runs_with_output(
        r#"
fn apply_first<T>(f fn(x T) T, v T) T
    return f(v)

fn apply_last<T>(v T, f fn(x T) T) T
    return f(v)

fn outer<U>(f fn(x U) U, v U) U
    return apply_first(f, v) + apply_last(v, f)

fn main()
    println(f"{outer(fn(x int) int: x + 2, 40)}")
"#,
        "84",
    );
}

/// The operation takes its left operand's type, as it does for two numbers
/// written out: `k * a` with `k int` is an `int` at every binding of `a`, and
/// `a * k` is whatever `a` is. Both orders give what the concrete code gives.
#[test]
fn arithmetic_with_a_parameter_gives_the_left_operands_type_in_either_order() {
    assert_runs_with_output(
        r#"
fn left_concrete<T>(a T, k int) int
    return k * a

fn left_parameter<T>(a T, k int) T
    return a * k

fn equals_500<T>(a T, k int) bool
    return k * a == 500

fn main()
    let a i8 = 100
    let k = 5
    let concrete_left = k * a
    let concrete_right = a * k
    println(f"{left_concrete(a, k)} {concrete_left}")
    println(f"{left_parameter(a, k)} {concrete_right}")
    println(f"{equals_500(a, k)} {k * a == 500}")
"#,
        "500 500\n-12 -12\ntrue true",
    );
}

#[test]
fn a_concrete_left_operand_is_not_read_as_the_parameter() {
    assert_compiler_error(
        r#"
fn mul<T>(a T, k int) T
    return k * a

fn main()
    let a i8 = 100
    println(f"{mul(a, 5)}")
"#,
        "expected T, got int",
    );
}

/// A number times a vector is a vector, so a body that read `2.0 * v` as the
/// number's type is refused where `v` is bound to one rather than handing on
/// a vector as an `f32`.
#[test]
fn a_site_whose_operator_gives_another_type_than_the_body_read_is_refused() {
    assert_compiler_error(
        r#"
use system.gpu.vector

fn scaled<T>(v T) f32
    let s f32 = 2.0
    return s * v

fn main()
    let r = scaled(Vec3<f32>(1.0, 2.0, 3.0))
    println(f"{r}")
"#,
        "but the generic body reads the result as f32",
    );
}

/// The operand beside a parameter is a type the body already knows, and one
/// with no ordering has none at any binding of the parameter: comparing it
/// would compare where the two values happen to be allocated.
#[test]
fn a_concrete_operand_without_an_ordering_is_refused_beside_a_parameter() {
    assert_compiler_error(
        r#"
class Dog
    n int

fn lt<T>(a T, b Dog) bool
    return b < a

fn main()
    let a = Dog(n: 1)
    let b = Dog(n: 2)
    println(f"{lt(a, b)} {lt(b, a)}")
"#,
        "Type 'Dog' has no ordering",
    );
}

#[test]
fn a_concrete_right_operand_without_an_ordering_is_refused_beside_a_parameter() {
    assert_compiler_error(
        r#"
class Dog
    n int

fn ge<T>(a T, b Dog) bool
    return a >= b

fn main()
    let a = Dog(n: 1)
    let b = Dog(n: 2)
    println(f"{ge(a, b)}")
"#,
        "Type 'Dog' has no ordering",
    );
}

/// A parameter on the right is bound to a type with no ordering: the call is
/// refused once, naming the capability.
#[test]
fn a_right_parameter_bound_to_a_type_without_an_ordering_is_refused_once() {
    let code = r#"
class Dog
    n int

fn above<T>(a T) bool
    return 10 < a

fn main()
    println(f"{above(Dog(n: 1))}")
"#;
    assert_compiler_error(code, "MER_TYP_075");
    let output = crate::utils::miri_check(code).output();
    assert_eq!(output.matches("error[").count(), 1, "{output}");
}

#[test]
fn a_parameter_ordered_on_either_side_of_a_number_runs() {
    assert_runs_with_output(
        r#"
fn above<T>(a T) bool
    return 10 < a

fn at_most<T>(a T, b T) bool
    return a <= b

fn main()
    println(f"{above(30)} {above(3)} {at_most(1, 2)} {at_most<int>(3, 2)}")
"#,
        "true false true false",
    );
}
