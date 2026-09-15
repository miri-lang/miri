// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A lambda written inside a generic body belongs to the instantiation it is
//! lowered in: each instantiation emits its own lambda body, typed at that
//! instantiation's arguments, under a symbol no other body shares.
//!
//! Strings are built at run time so an address comparison or an unretained
//! reference cannot pass for the right answer.

use super::utils::*;

#[test]
fn test_a_lambda_inside_a_generic_function_runs() {
    assert_runs_with_output(
        r#"
fn apply_outer<T>(a T, b T) T
    let f = fn(x T, y T) T: x
    return f(a, b)

fn main()
    println(apply_outer("PEAR".to_lower(), "APPLE".to_lower()))
"#,
        "pear",
    );
}

#[test]
fn test_a_lambda_runs_at_every_type_its_generic_function_is_called_at() {
    assert_runs_with_output(
        r#"
fn second<T>(a T, b T) T
    let f = fn(x T, y T) T: y
    return f(a, b)

fn main()
    println(f"{second(3, 40)}")
    println(second("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{second(5, 60)}")
"#,
        "40\napple\n60",
    );
}

#[test]
fn test_a_lambda_calling_a_generic_function_orders_strings_by_content() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn pick<T>(a T, b T) T
    let f = fn(x T, y T) T: smaller(x, y)
    return f(a, b)

fn main()
    println(pick("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{pick(9, 2)}")
"#,
        "apple\n2",
    );
}

#[test]
fn test_a_lambda_capturing_a_generic_value_reads_it_at_each_type() {
    assert_runs_with_output(
        r#"
fn keep_first<T>(a T, b T) T
    let f = fn(y T) T: a
    return f(b)

fn main()
    println(keep_first("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{keep_first(7, 8)}")
"#,
        "pear\n7",
    );
}

#[test]
fn test_a_lambda_nested_in_a_lambda_inside_a_generic_function_runs() {
    assert_runs_with_output(
        r#"
fn twice<T>(a T, b T) T
    let outer = fn(x T, y T) T
        let inner = fn(p T, q T) T: q
        return inner(y, x)
    return outer(a, b)

fn main()
    println(twice("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{twice(1, 2)}")
"#,
        "pear\n1",
    );
}

#[test]
fn test_a_lambda_over_two_type_parameters_runs_at_each_pairing() {
    assert_runs_with_output(
        r#"
fn first_of<A, B>(a A, b B) A
    let f = fn(x A, _y B) A: x
    return f(a, b)

fn main()
    let s = "X".to_lower()
    println(f"{first_of(3, s)}")
    println(first_of("PEAR".to_lower(), 4))
"#,
        "3\npear",
    );
}

#[test]
fn test_a_function_nested_in_a_generic_function_runs_at_each_type() {
    assert_runs_with_output(
        r#"
fn pick<T>(a T, b T) T
    fn chooser(_x T, y T) T
        return y
    return chooser(a, b)

fn main()
    println(pick("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{pick(1, 2)}")
"#,
        "apple\n2",
    );
}

#[test]
fn test_a_function_reference_inside_a_generic_function_runs_at_each_type() {
    assert_runs_with_output(
        r#"
fn shout(s String) String
    return s.to_upper()

fn tag<T>(a T) T
    let g = shout
    println(g("hi".to_lower()))
    return a

fn main()
    println(f"{tag(5)}")
    println(tag("PEAR".to_lower()))
"#,
        "HI\n5\nHI\npear",
    );
}

#[test]
fn test_a_lambda_in_a_generic_class_method_runs_at_each_instantiation() {
    assert_runs_with_output(
        r#"
class Box<T>
    var item T

    fn pick(self, other T) T
        let f = fn(_x T, y T) T: y
        return f(self.item, other)

fn main()
    let a = Box<int>(1)
    println(f"{a.pick(2)}")
    let b = Box<String>("PEAR".to_lower())
    println(b.pick("APPLE".to_lower()))
"#,
        "2\napple",
    );
}
