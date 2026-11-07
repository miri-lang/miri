// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter written as the type argument of a bounded generic class.
//!
//! Inside a generic body a parameter stands for whatever meets its own bound,
//! so it meets a class's bound exactly when its own bound does: a parameter
//! bounded by the required trait, or by a trait extending it, is accepted; a
//! type-set parameter is accepted when every member of the set is. A
//! parameter with no bound, or with a bound that does not entail the
//! required one, is refused where the argument is written.

use super::utils::*;
use crate::utils::{miri_check, strip_ansi};

const NAMED_BOX: &str = r#"
use system.io

trait Named
    fn name() String

class P implements Named
    fn name() String
        return "p"

class Q
    fn tag() int
        return 1

class Box<T implements Named>
    v T
"#;

/// `check` output with colour escapes removed, for tests that read locations.
fn check_output(code: &str) -> String {
    strip_ansi(&miri_check(code).output())
}

/// The line and column of the one place `written` occurs in `code`.
fn line_and_column(code: &str, written: &str) -> (usize, usize) {
    let mut found = code.match_indices(written);
    let (offset, _) = found
        .next()
        .unwrap_or_else(|| panic!("`{written}` is not in the program:\n{code}"));
    assert!(
        found.next().is_none(),
        "`{written}` occurs more than once in the program:\n{code}"
    );
    let before = &code[..offset];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    (before.matches('\n').count() + 1, offset - line_start + 1)
}

/// Asserts `code` reports MER_TYP_037 with `message` at `line:column`.
fn assert_refused_at(code: &str, message: &str, line: usize, column: usize) {
    let output = check_output(code);
    assert!(
        output.contains("MER_TYP_037"),
        "expected MER_TYP_037:\n{output}"
    );
    assert!(output.contains(message), "expected '{message}':\n{output}");
    let location = format!(":{line}:{column}");
    assert!(
        output.contains(&location),
        "expected MER_TYP_037 at {location}:\n{output}"
    );
}

/// Asserts `code` refuses the argument of the one `class_type` it writes
/// (`Box<T>`) with `message`, reported where that argument starts.
fn assert_argument_refused(code: &str, message: &str, class_type: &str) {
    let (line, column) = line_and_column(code, class_type);
    let argument_offset = class_type.find('<').map_or(0, |open| open + 1);
    assert_refused_at(code, message, line, column + argument_offset);
}

/// Asserts `code` refuses the one call `written` with `message`, reported
/// where the call starts.
fn assert_call_refused(code: &str, message: &str, written: &str) {
    let (line, column) = line_and_column(code, written);
    assert_refused_at(code, message, line, column);
}

#[test]
fn a_parameter_bounded_by_the_required_trait_is_a_class_argument() {
    assert_runs_with_output(
        &format!(
            "{NAMED_BOX}{}",
            r#"
fn wrap<T implements Named>(x T) String
    let b = Box<T>(v: x)
    return b.v.name()

fn main()
    println(wrap(P()))
"#
        ),
        "p",
    );
}

#[test]
fn a_parameter_bounded_by_an_extending_trait_is_a_class_argument() {
    assert_runs_with_output(
        &format!(
            "{NAMED_BOX}{}",
            r#"
trait Titled extends Named
    fn title() String

class R implements Titled
    fn name() String
        return "r"
    fn title() String
        return "Dr"

fn wrap<T implements Titled>(x T) String
    let b = Box<T>(v: x)
    return f"{b.v.title()} {b.v.name()}"

fn main()
    println(wrap(R()))
"#
        ),
        "Dr r",
    );
}

#[test]
fn a_parameter_bounded_by_a_trait_extending_hashable_is_a_class_argument() {
    assert_runs_with_output(
        r#"
use system.io

class Box<T implements Hashable>
    v T

trait Key extends Hashable
    fn k() int

fn wrap<T implements Key>(x T) int
    let b = Box<T>(v: x)
    return b.v.k()

fn wrap_any<T implements Hashable>(x T) Box<T>
    return Box<T>(v: x)

fn main()
    let b = wrap_any(3)
    println(f"{b.v}")
"#,
        "3",
    );
}

#[test]
fn a_parameter_bounded_by_equatable_is_a_class_argument() {
    assert_runs_with_output(
        r#"
use system.io

class Box<T implements Equatable>
    v T

fn same<T implements Equatable>(x T, y T) bool
    let b = Box<T>(v: x)
    return b.v == y

fn main()
    let a = "a"
    let b = "b"
    println(f"{same(2, 2)} {same(a, b)}")
"#,
        "true false",
    );
}

#[test]
fn a_type_set_parameter_whose_members_all_meet_the_bound_is_a_class_argument() {
    assert_runs_with_output(
        r#"
use system.io

class Box<T implements Hashable>
    v T

fn wrap<T is int or String>(x T) T
    let b = Box<T>(v: x)
    return b.v

fn main()
    let s = "s"
    println(f"{wrap(4)} {wrap(s)}")
"#,
        "4 s",
    );
}

#[test]
fn an_unbounded_parameter_is_refused_where_it_is_written() {
    let code = format!(
        "{NAMED_BOX}{}",
        r#"
fn wrap<T>(x T) int
    let b = Box<T>(v: x)
    return 1

fn main()
    println(f"{wrap(P())}")
"#
    );
    assert_argument_refused(
        &code,
        "Type T does not satisfy constraint implements Named",
        "Box<T>",
    );
}

#[test]
fn a_parameter_bounded_by_an_unrelated_trait_is_refused() {
    let code = format!(
        "{NAMED_BOX}{}",
        r#"
trait Sized
    fn size() int

fn wrap<T implements Sized>(x T) int
    let b = Box<T>(v: x)
    return 1

fn main()
    println("unreachable")
"#
    );
    assert_argument_refused(
        &code,
        "Type T does not satisfy constraint implements Named",
        "Box<T>",
    );
}

#[test]
fn a_type_set_parameter_with_a_member_missing_the_bound_is_refused() {
    let code = format!(
        "{NAMED_BOX}{}",
        r#"
fn wrap<T is P or Q>(x T) int
    let b = Box<T>(v: x)
    return 1

fn main()
    println("unreachable")
"#
    );
    assert_argument_refused(
        &code,
        "Type T does not satisfy constraint implements Named",
        "Box<T>",
    );
}

#[test]
fn a_refused_type_argument_is_spelled_as_written() {
    let code = format!(
        "{NAMED_BOX}{}",
        r#"
fn main()
    let b = Box<List<int>>(v: List<int>())
"#
    );
    assert_argument_refused(
        &code,
        "Type List<int> does not satisfy constraint implements Named",
        "Box<List<int>>",
    );
}

#[test]
fn a_class_parameter_meets_another_class_bound_in_its_own_method() {
    assert_runs_with_output(
        &format!(
            "{NAMED_BOX}{}",
            r#"
class W<T implements Named>
    v T
    fn boxed_name() String
        let b = Box<T>(v: self.v)
        return b.v.name()

fn main()
    let w = W<P>(v: P())
    println(w.boxed_name())
"#
        ),
        "p",
    );
}

#[test]
fn a_parameter_bounded_by_a_supertrait_is_refused() {
    let code = r#"
trait Named
    fn name() String

trait Titled extends Named
    fn title() String

class Box<T implements Titled>
    v T

fn wrap<T implements Named>(x T) int
    let b = Box<T>(v: x)
    return 1

fn main()
    let n = 0
"#;
    assert_argument_refused(
        code,
        "Type T does not satisfy constraint implements Titled",
        "Box<T>",
    );
}

#[test]
fn a_type_set_within_the_required_set_is_a_class_argument() {
    assert_runs_with_output(
        r#"
use system.io

class Box<T is int or String>
    v T

fn wrap<T is int>(x T) T
    let b = Box<T>(v: x)
    return b.v

fn main()
    println(f"{wrap(7)}")
"#,
        "7",
    );
}

#[test]
fn a_type_set_wider_than_the_required_set_is_refused() {
    let code = r#"
class Box<T is int or String>
    v T

fn wrap<T is int or String or f64>(x T) int
    let b = Box<T>(v: x)
    return 1

fn main()
    let n = 0
"#;
    assert_argument_refused(
        code,
        "Type T does not satisfy constraint is int or String",
        "Box<T>",
    );
}

const NAMED_CALLEE: &str = r#"
use system.io

trait Named
    fn name() String

class P implements Named
    fn name() String
        return "p"

class R implements Named
    fn name() String
        return "r"

fn named<U implements Named>(x U) String
    return x.name()
"#;

#[test]
fn a_type_set_argument_whose_members_all_meet_a_call_bound_is_accepted() {
    assert_runs_with_output(
        &format!(
            "{NAMED_CALLEE}{}",
            r#"
fn relay<T is P or R>(x T) String
    return named(x)

fn main()
    println(f"{relay(P())} {relay(R())}")
"#
        ),
        "p r",
    );
}

#[test]
fn a_type_set_argument_with_a_member_missing_a_call_bound_is_refused() {
    let code = format!(
        "{NAMED_CALLEE}{}",
        r#"
fn relay<T is P or int>(x T) String
    return named(x)

fn main()
    println("unreachable")
"#
    );
    assert_call_refused(
        &code,
        "Type T does not satisfy constraint implements Named",
        "named(x)",
    );
}

#[test]
fn a_written_annotation_argument_is_refused_where_it_is_written() {
    let code = format!(
        "{NAMED_BOX}{}",
        r#"
fn unwrap(b Box<Q>) int
    return 1

fn main()
    println("unreachable")
"#
    );
    assert_argument_refused(
        &code,
        "Type Q does not satisfy constraint implements Named",
        "Box<Q>",
    );
}
