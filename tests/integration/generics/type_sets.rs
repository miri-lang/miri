// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type sets: `type Real is f32 or float` bounds a type parameter to one of
//! its members, written as a bound (`<T is Real>`, `<T is f32 or float>`) or as
//! a parameter type (`fn sq(x Real) Real`), and is never the type of a value.

use super::utils::*;

#[test]
fn a_named_set_bounds_a_parameter_to_each_member() {
    assert_runs_with_output(
        r#"
type Real is f32 or float

fn sq<T is Real>(x T) T
    return x * x

fn main()
    let a f32 = 1.1
    println(f"{sq(a)} {sq(1.1)}")
"#,
        "1.21 1.2100000000000002",
    );
}

#[test]
fn an_inline_set_bounds_a_parameter() {
    assert_runs_with_output(
        r#"
fn sq<T is f32 or float>(x T) T
    return x * x

fn main()
    let a f32 = 1.1
    println(f"{sq(a)} {sq(1.1)}")
"#,
        "1.21 1.2100000000000002",
    );
}

#[test]
fn a_set_written_as_a_parameter_type_returns_the_argument_type() {
    assert_runs_with_output(
        r#"
type Real is f32 or float

fn sq(x Real) Real
    return x * x

fn main()
    let a f32 = 1.1
    let r f32 = sq(a)
    println(f"{r} {sq(1.1)}")
"#,
        "1.21 1.2100000000000002",
    );
}

#[test]
fn every_use_of_one_set_name_in_a_signature_is_the_same_type() {
    assert_runs_with_output(
        r#"
type Real is f32 or float

fn limit(x Real, lo Real, hi Real) Real
    if x < lo
        return lo
    if x > hi
        return hi
    return x

fn main()
    let a f32 = 1.5
    let r f32 = limit(a, 0.0, 1.0)
    println(f"{r} {limit(-2.5, 0.0, 1.0)}")
"#,
        "1.0 0.0",
    );
}

#[test]
fn arguments_of_one_set_name_at_different_members_are_refused() {
    assert_compiler_error(
        r#"
type Real is f32 or float

fn add(x Real, y Real) Real
    return x + y

fn main()
    let a f32 = 1.5
    let b float = 2.0
    let s = add(a, b)
"#,
        "expected f32, got float",
    );
}

#[test]
fn a_type_outside_the_set_is_refused_at_the_call() {
    assert_compiler_error(
        r#"
type Real is f32 or float

fn sq(x Real) Real
    return x * x

fn main()
    let s = sq("a")
"#,
        "String is not one of the types Real accepts: f32 or float",
    );
    assert_compiler_error(
        r#"
fn sq<T is f32 or float>(x T) T
    return x * x

fn main()
    let n = sq(3)
"#,
        "int is not one of the types T accepts: f32 or float",
    );
}

#[test]
fn a_set_may_hold_another_set() {
    assert_runs_with_output(
        r#"
type Real is f32 or float
type Number is Real or int

fn sq(x Number) Number
    return x * x

fn main()
    let k f32 = 2.0
    println(f"{sq(3)} {sq(k)} {sq(1.5)}")
"#,
        "9 4.0 2.25",
    );
}

#[test]
fn a_set_bounded_parameter_passes_on_to_a_wider_set() {
    assert_runs_with_output(
        r#"
type Real is f32 or float
type Number is Real or int

fn twice(x Number) Number
    return x + x

fn thrice(x Real) Real
    return twice(x) + x

fn main()
    let a f32 = 1.5
    let r f32 = thrice(a)
    println(f"{r}")
"#,
        "4.5",
    );
}

#[test]
fn a_parameter_bounded_by_a_wider_set_is_refused_by_a_narrower_one() {
    assert_compiler_error(
        r#"
type Real is f32 or float
type Number is Real or int

fn half(x Real) Real
    return x * 0.5

fn quarter(x Number) Number
    return half(half(x))

fn main()
    let q = quarter(1.0)
"#,
        "is not one of the types Real accepts: f32 or float",
    );
}

#[test]
fn written_type_arguments_bind_the_set_parameter() {
    assert_runs_with_output(
        r#"
type Real is f32 or float

fn add(x Real, y Real) Real
    return x + y

fn main()
    let r = add<f32>(0.1, 0.2)
    println(f"{r == 0.3}")
"#,
        "true",
    );
}

#[test]
fn a_set_is_not_the_type_of_a_value() {
    let refused = "is a type set (f32 or float), which bounds a type parameter and cannot be the type of a value";
    assert_compiler_error(
        r#"
type Real is f32 or float

fn main()
    let y Real = 1.0
"#,
        refused,
    );
    assert_compiler_error(
        r#"
type Real is f32 or float

struct P
    r Real

fn main()
    println("x")
"#,
        refused,
    );
    assert_compiler_error(
        r#"
type Real is f32 or float

fn main()
    let xs [Real] = [1.0]
"#,
        refused,
    );
    assert_compiler_error(
        r#"
type Real is f32 or float

fn main()
    let f = fn(x Real) Real: x
"#,
        refused,
    );
}

#[test]
fn the_body_of_a_shorthand_function_reads_the_set_name_as_the_set() {
    assert_compiler_error(
        r#"
type Real is f32 or float

fn copy(x Real) Real
    let y Real = x
    return y

fn main()
    let c = copy(1.0)
"#,
        "'Real' is a type set (f32 or float)",
    );
}

#[test]
fn a_method_cannot_take_a_set() {
    assert_compiler_error(
        r#"
type Real is f32 or float

class Scale
    k float
    fn by(x Real) Real
        return x

fn main()
    let s = Scale(k: 2.0)
"#,
        "'Real' is a type set (f32 or float)",
    );
}

#[test]
fn a_set_and_its_shorthand_functions_are_imported_with_their_module() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.shapes\n",
                    "fn thrice(x Real) Real\n",
                    "    return twice(x) + x\n",
                    "fn main()\n",
                    "    let a f32 = 1.5\n",
                    "    let r f32 = thrice(a)\n",
                    "    println(f\"{r} {twice(2.0)}\")\n",
                ),
            ),
            (
                "shapes.mi",
                concat!(
                    "public type Real is f32 or float\n",
                    "public fn twice(x Real) Real\n",
                    "    return x * 2.0\n",
                ),
            ),
        ],
        "4.5 4.0",
    );
}

#[test]
fn a_set_is_imported_by_name() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.shapes.{twice, Real}\n",
                    "fn half(x Real) Real\n",
                    "    return x * 0.5\n",
                    "fn main()\n",
                    "    let a f32 = 3.0\n",
                    "    let r f32 = half(twice(a))\n",
                    "    println(f\"{r}\")\n",
                ),
            ),
            (
                "shapes.mi",
                concat!(
                    "public type Real is f32 or float\n",
                    "public fn twice(x Real) Real\n",
                    "    return x * 2.0\n",
                ),
            ),
        ],
        "3.0",
    );
}
