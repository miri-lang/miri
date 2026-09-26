// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Two definitions the compiler links under one name. A compiled name joins
//! user identifiers with `_` and `__`, which an identifier may itself contain,
//! so distinct definitions can spell the same name; every such pair is
//! refused, never resolved by quietly running one definition's body for the
//! other.

use super::utils::*;

#[test]
fn generic_instance_beside_a_function_spelled_alike() {
    assert_build_error(
        r#"
use system.io

fn pick<T>(x T) T
    return x

fn pick__int(x int) int
    return x + 1000

fn main()
    println(f"{pick<int>(7)} {pick__int(7)}")
    println(f"{pick(5)}")
"#,
        "`pick__int` and `pick<int>` compile to the same symbol `pick__int`",
    );
}

#[test]
fn function_declared_before_a_generic_instance_spelled_alike() {
    assert_build_error(
        r#"
use system.io

fn pick__int(x int) int
    return x + 1000

fn pick<T>(x T) T
    return x

fn main()
    println(f"{pick(5)} {pick__int(7)}")
"#,
        "`pick__int` and `pick<int>` compile to the same symbol `pick__int`",
    );
}

#[test]
fn generic_instance_beside_a_static_method_spelled_alike() {
    assert_build_error(
        r#"
use system.io

class Pick
    public static fn _int(x int) int
        return x + 1000

fn Pick<T>(x T) T
    return x

fn main()
    println(f"{Pick<int>(7)}")
"#,
        "`Pick._int` and `Pick<int>` compile to the same symbol `Pick__int`",
    );
}

#[test]
fn methods_of_classes_whose_names_run_together() {
    assert_build_error(
        r#"
use system.io

class A_b
    fn c() int
        return 1

class A
    fn b_c() int
        return 2

fn main()
    let x = A_b()
    let y = A()
    println(f"{x.c()} {y.b_c()}")
"#,
        "`A_b.c` and `A.b_c` compile to the same symbol `A_b_c`",
    );
}

#[test]
fn function_declared_before_a_method_spelled_alike() {
    assert_build_error(
        r#"
use system.io

fn Point_norm(p Point) int
    return 2

class Point
    x int
    fn norm() int
        return 1

fn main()
    let p = Point(x: 1)
    println(f"{p.norm()}")
"#,
        "`Point_norm` and `Point.norm` compile to the same symbol `Point_norm`",
    );
}

#[test]
fn function_declared_after_a_method_spelled_alike() {
    assert_build_error(
        r#"
use system.io

class Point
    fn norm() int
        return 1

fn Point_norm(p Point) int
    return 2

fn main()
    let p = Point()
    println(f"{p.norm()} {Point_norm(p)}")
"#,
        "`Point.norm` and `Point_norm` compile to the same symbol `Point_norm`",
    );
}

#[test]
fn symbol_collision_names_its_diagnostic_code() {
    assert_build_error(
        r#"
use system.io

class A_b
    fn c() int
        return 1

class A
    fn b_c() int
        return 2

fn main()
    println(f"{A_b().c()} {A().b_c()}")
"#,
        "MER_MIR_018",
    );
}
