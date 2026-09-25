// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic method reached through `extends` or `implements`, where the
//! receiver's own class pins the declaring type's parameters in its clause
//! rather than at the call. What the body requires of its parameter is judged
//! at the type the clause binds it to.

use super::utils::*;

#[test]
fn sorting_in_an_inherited_method_is_refused_at_an_unordered_struct() {
    assert_compiler_error(
        r#"
use system.collections.list

struct Pt
    x int

class Base<T>
    fn first_sorted(a T, b T) T
        var l = List<T>()
        l.push(a)
        l.push(b)
        l.sort()
        return l[0]

class Sub extends Base<Pt>

fn main()
    let p = Sub().first_sorted(Pt(x: 2), Pt(x: 1))
    println(f"{p.x}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn arithmetic_in_an_inherited_method_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

class Base<T>
    fn add(a T, b T) T
        return a + b

class Sub extends Base<Pt>

fn main()
    let p = Sub().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_two_extends_clauses_up_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

class Base<T>
    fn add(a T, b T) T
        return a + b

class Mid<U> extends Base<U>

class Sub extends Mid<Pt>

fn main()
    let p = Sub().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_through_a_renamed_parameter_is_refused_where_the_subclass_is_pinned() {
    assert_compiler_error(
        r#"
struct Pt
    x int

class Base<T>
    fn add(a T, b T) T
        return a + b

class Sub<U> extends Base<U>

fn main()
    let p = Sub<Pt>().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_inherited_arithmetic_runs_at_the_type_the_clause_binds() {
    assert_runs_with_output(
        r#"
class Base<T>
    fn add(a T, b T) T
        return a + b

class Mid<U> extends Base<U>

class Sub extends Mid<int>

fn main()
    println(f"{Sub().add(20, 22)}")
"#,
        "42",
    );
}

#[test]
fn test_inherited_sort_runs_at_an_ordered_type() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Base<T>
    fn first_sorted(a T, b T) T
        var l = List<T>()
        l.push(a)
        l.push(b)
        l.sort()
        return l[0]

class Sub extends Base<int>

fn main()
    println(f"{Sub().first_sorted(9, 4)}")
"#,
        "4",
    );
}

#[test]
fn arithmetic_in_a_trait_default_is_refused_where_a_plain_class_binds_the_trait() {
    assert_compiler_error(
        r#"
struct Pt
    x int

trait Op<T>
    fn add(a T, b T) T
        return a + b

class Impl implements Op<Pt>

fn main()
    let p = Impl().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_in_a_generic_enum_method_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

enum Pair<T>
    Both(T, T)

    fn sum() T
        match self
            Pair.Both(a, b): a + b

fn main()
    let p Pair<Pt> = Pair.Both(Pt(x: 1), Pt(x: 2))
    println(f"{p.sum().x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_arithmetic_in_a_generic_enum_method_runs_at_int() {
    assert_runs_with_output(
        r#"
enum Pair<T>
    Both(T, T)

    fn sum() T
        match self
            Pair.Both(a, b): a + b

fn main()
    let p Pair<int> = Pair.Both(20, 22)
    println(f"{p.sum()}")
"#,
        "42",
    );
}

#[test]
fn arithmetic_in_a_trait_default_is_refused_through_a_trait_typed_receiver() {
    assert_compiler_error(
        r#"
struct Pt
    x int

trait Op<T>
    fn add(a T, b T) T
        return a + b

class Impl<T> implements Op<T>

fn main()
    let o Op<Pt> = Impl<Pt>()
    let p = o.add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

/// The call's type is read at the receiver's type argument, so the result is
/// an `int` that prints, not a bare `T` with no members.
#[test]
fn test_a_trait_default_through_a_trait_typed_receiver_returns_the_bound_type() {
    assert_runs_with_output(
        r#"
struct Pt
    x int

trait Op<T>
    fn keep(a T) T
        return a

class Impl<T> implements Op<T>

fn main()
    let o Op<Pt> = Impl<Pt>()
    let p = o.keep(Pt(x: 42))
    println(f"{p.x}")
"#,
        "42",
    );
}
