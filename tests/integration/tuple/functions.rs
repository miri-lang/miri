// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_passed_to_function() {
    assert_runs_with_output(
        r#"
use system.io

fn print_tuple(t (int, String))
    println(f"{t.0} {t.1}")

fn main()
    let t = (42, "hello")
    print_tuple(t: t)
"#,
        "42 hello",
    );
}

#[test]
fn test_tuple_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io

fn make_tuple() (int, bool)
    return (99, true)

fn main()
    let t = make_tuple()
    println(f"{t.0} {t.1}")
"#,
        "99 true",
    );
}

#[test]
fn test_tuple_passed_to_function_with_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

fn sum_tuple(t (int, int, int)) int
    var total = 0
    for x in t
        total = total + x
    return total

fn main()
    let t = (10, 20, 30)
    println(f"{sum_tuple(t: t)}")
"#,
        "60",
    );
}

#[test]
fn test_tuple_returned_from_function_with_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

fn make_triple() (int, int, int)
    return (5, 10, 15)

fn main()
    let t = make_triple()
    println(f"{t.length()}")
    println(f"{t.contains(10)}")
"#,
        "3\ntrue",
    );
}
