// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `T?` is the enum the standard library declares `@optional`. The marker
//! is checked where it is written: the enum must have the optional's shape,
//! and only one enum may carry it. Each shape rule is refused on its own,
//! and an enum of the same shape without the marker stays an ordinary enum.

use super::super::utils::*;

const SHAPE_HELP: &str = "with one variant without a payload and one carrying `T`";

/// Assert `code` is refused with MER_TYP_092, the message carrying `reason`.
fn assert_optional_refused(code: &str, reason: &str) {
    assert_compiler_error(code, "error[MER_TYP_092]");
    assert_compiler_error(code, reason);
}

#[test]
fn test_a_second_enum_marked_optional_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(T)

fn main()
    let m = Maybe.Just(1)
"#,
        "'Maybe' is marked `@optional`, and 'Option' already is: `T?` names one type",
    );
}

#[test]
fn test_an_optional_with_two_type_parameters_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Pair<T, U>
    Nothing
    Both(T)

fn main()
    println("x")
"#,
        "it declares 2 type parameters, and the optional declares one",
    );
}

#[test]
fn test_an_optional_with_no_type_parameter_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Flag
    Off
    On(int)

fn main()
    println("x")
"#,
        "it declares 0 type parameters, and the optional declares one",
    );
}

#[test]
fn test_an_optional_with_three_variants_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(T)
    Unknown

fn main()
    println("x")
"#,
        "it declares 3 variants, and the optional declares two",
    );
}

#[test]
fn test_an_optional_with_no_absent_variant_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Left(T)
    Right(T)

fn main()
    println("x")
"#,
        "none of its variants is without a payload",
    );
}

#[test]
fn test_an_optional_whose_payload_is_not_its_parameter_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(int)

fn main()
    println("x")
"#,
        "none of its variants carries exactly its type parameter",
    );
}

#[test]
fn test_an_optional_with_a_drop_hook_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(T)

    fn drop()
        println("dropped")

fn main()
    println("x")
"#,
        "it declares `drop`, and the optional's drop is the compiler's",
    );
}

#[test]
fn test_an_optional_with_its_own_equals_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(T)

    fn equals(other Maybe<T>) bool
        return true

fn main()
    println("x")
"#,
        "it declares `equals`",
    );
}

#[test]
fn test_an_optional_with_its_own_hash_is_refused() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(T)

    fn hash() u64
        return 0

fn main()
    println("x")
"#,
        "it declares `hash`",
    );
}

#[test]
fn test_a_non_exhaustive_optional_is_refused() {
    assert_optional_refused(
        r#"
@optional
@non_exhaustive
enum Maybe<T>
    Nothing
    Just(T)

fn main()
    println("x")
"#,
        "it is `@non_exhaustive`, and the optional's two variants are all it has",
    );
}

#[test]
fn test_a_refused_shape_names_the_shape_to_declare() {
    assert_optional_refused(
        r#"
@optional
enum Maybe<T>
    Nothing
    Just(int)

fn main()
    println("x")
"#,
        SHAPE_HELP,
    );
}

#[test]
fn test_the_optional_marker_is_refused_on_a_class() {
    assert_compiler_error(
        r#"
@optional
class Box
    var n int

fn main()
    println("x")
"#,
        "Attribute @optional is not valid on a class declaration",
    );
}

#[test]
fn test_the_optional_marker_is_refused_on_a_function() {
    assert_compiler_error(
        r#"
@optional
fn f()
    return

fn main()
    f()
"#,
        "Attribute @optional is not valid on a function declaration",
    );
}

#[test]
fn test_the_optional_marker_is_refused_on_a_struct() {
    assert_compiler_error(
        r#"
@optional
struct P
    x int

fn main()
    println("x")
"#,
        "error[MER_PAR_019]",
    );
}

#[test]
fn test_a_second_enum_marked_optional_in_an_imported_module_is_refused() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                "use local.maybe\n\nfn main()\n    let m = Maybe.Just(1)\n",
            ),
            (
                "maybe.mi",
                "@optional\npublic enum Maybe<T>\n    Nothing\n    Just(T)\n",
            ),
        ],
        "'Maybe' is marked `@optional`, and 'Option' already is: `T?` names one type",
    );
}

#[test]
fn test_an_enum_of_the_optional_shape_without_the_marker_is_an_ordinary_enum() {
    assert_runs_with_output(
        r#"
use system.io

enum Maybe<T>
    Nothing
    Just(T)

fn main()
    let m = Maybe.Just(4)
    let o int? = Some(5)
    match m
        Maybe.Just(n): println(f"{n} {o ?? 0}")
        Maybe.Nothing: println("nothing")
"#,
        "4 5",
    );
}

#[test]
fn test_a_program_cannot_declare_its_own_option() {
    assert_compiler_error(
        r#"
enum Option<T>
    Some(T)
    None

fn main()
    println("x")
"#,
        "Type 'Option' is already defined",
    );
}
