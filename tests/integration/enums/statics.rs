// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_static_scalar() {
    assert_runs_with_output(
        r#"
enum Val
    Num(int)
    public static fn make() Val
        Val.Num(42)

fn main()
    let v = Val.make()
    match v
        Val.Num(n): println(f"{n}")
"#,
        "42",
    );
}

#[test]
fn test_enum_static_managed_return() {
    assert_runs_with_output(
        r#"
enum StringWrapper
    Empty
    Some(String)
    public static fn create(s String) StringWrapper
        StringWrapper.Some(s)

fn main()
    let w = StringWrapper.create("hello")
    match w
        StringWrapper.Some(s): println(s)
        StringWrapper.Empty: println("empty")
"#,
        "hello",
    );
}

#[test]
fn test_enum_static_own_type() {
    assert_runs_with_output(
        r#"
enum Outcome
    Success(int)
    Failure(String)
    public static fn ok(value int) Outcome
        Outcome.Success(value)
    public static fn err(msg String) Outcome
        Outcome.Failure(msg)

fn main()
    let r = Outcome.ok(10)
    match r
        Outcome.Success(v): println(f"{v}")
        Outcome.Failure(_): println("failed")
"#,
        "10",
    );
}

#[test]
fn test_enum_static_bare() {
    assert_runs_with_output(
        r#"
enum Foo
    Value
    static fn secret() int
        42

fn main()
    println(f"{Foo.secret()}")
"#,
        "42",
    );
}

#[test]
fn test_enum_phantom_static_variant_rejected() {
    assert_compiler_error(
        r#"
enum Foo
    Value
    static fn secret() int
        0

fn main()
    let x = Foo.static
"#,
        "has no variant",
    );
}

#[test]
fn test_enum_static_private_method() {
    assert_compiler_error(
        r#"
enum Foo
    Value
    private static fn secret() int
        0

fn main()
    let x = Foo.secret()
"#,
        "cannot be accessed",
    );
}

#[test]
fn test_enum_variant_static_collision() {
    assert_compiler_error(
        r#"
enum Bad
    make
    public static fn make() Bad
        Bad.make
"#,
        "collision",
    );
}

#[test]
fn test_enum_variant_takes_precedence() {
    assert_runs_with_output(
        r#"
enum Status
    Active
    Inactive

fn main()
    let s = Status.Active
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_enum_static_no_self_param() {
    assert_compiler_error(
        r#"
enum E
    Value
    public static fn bad(self E) int
        0
"#,
        "cannot",
    );
}

#[test]
fn test_enum_static_no_async() {
    assert_compiler_error(
        r#"
enum E
    Value
    public static async fn bad() int
        0
"#,
        "cannot",
    );
}

#[test]
fn test_enum_static_no_gpu() {
    assert_compiler_error(
        r#"
enum E
    Value
    public static gpu fn bad() int
        0
"#,
        "cannot",
    );
}
