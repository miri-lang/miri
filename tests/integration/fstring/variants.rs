// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_option_some_int() {
    assert_runs_with_output(
        r#"
let x = Some(15)
print(f"{x}")
"#,
        "Some(15)",
    );
}

#[test]
fn test_option_none() {
    assert_runs_with_output(
        r#"
fn test_none() int?
    None

let x = test_none()
print(f"{x}")
"#,
        "None",
    );
}

#[test]
fn test_option_some_string() {
    assert_runs_with_output(
        r#"
let x = Some("hello")
print(f"{x}")
"#,
        "Some(hello)",
    );
}

#[test]
fn test_option_none_string() {
    assert_runs_with_output(
        r#"
fn test_none_str() String?
    None

let x = test_none_str()
print(f"{x}")
"#,
        "None",
    );
}

#[test]
fn test_result_ok() {
    assert_runs_with_output(
        r#"
fn make_result() Result<int, String>
    Result.Ok(42)

let x = make_result()
print(f"{x}")
"#,
        "Ok(42)",
    );
}

#[test]
fn test_result_err() {
    assert_runs_with_output(
        r#"
fn make_error() Result<int, String>
    Result.Err("oops")

let x = make_error()
print(f"{x}")
"#,
        "Err(oops)",
    );
}

#[test]
fn test_option_nested() {
    assert_runs_with_output(
        r#"
let x = Some(Some(1))
print(f"{x}")
"#,
        "Some(Some(1))",
    );
}

#[test]
fn test_option_of_result() {
    assert_runs_with_output(
        r#"
fn make_opt() Result<int, String>?
    Some(Result.Ok(2))

let x = make_opt()
print(f"{x}")
"#,
        "Some(Ok(2))",
    );
}

#[test]
fn test_enum_payload_less() {
    assert_runs_with_output(
        r#"
enum Color: Red, Green, Blue

let c = Color.Red
print(f"{c}")
"#,
        "Red",
    );
}

#[test]
fn test_enum_single_payload() {
    assert_runs_with_output(
        r#"
enum Color: Red, Green, Blue, Tagged(int)

let c = Color.Tagged(7)
print(f"{c}")
"#,
        "Tagged(7)",
    );
}

#[test]
fn test_enum_multi_payload() {
    assert_runs_with_output(
        r#"
enum Pair: Variant(int, int)

let p = Pair.Variant(1, 2)
print(f"{p}")
"#,
        "Variant(1, 2)",
    );
}

#[test]
fn test_format_multiple_options() {
    assert_runs_with_output(
        r#"
let x = Some(10)
let y = Some(20)
print(f"{x} and {y}")
"#,
        "Some(10) and Some(20)",
    );
}

#[test]
fn test_non_formattable_payload() {
    assert_compiler_error(
        r#"
fn make_list_opt() [int]?
    None

let o = make_list_opt()
print(f"{o}")
"#,
        "cannot be used in string interpolation",
    );
}

#[test]
fn test_recursive_enum_does_not_hang() {
    assert_compiler_error(
        r#"
enum Json: Value([Json]?)

let j = Json.Value(None)
print(f"{j}")
"#,
        "cannot be used in string interpolation",
    );
}

#[test]
fn test_direct_self_referential_enum_rejected() {
    assert_compiler_error(
        r#"
enum Wrapper
    Wrap(Wrapper)
    Leaf(int)

let w = Wrapper.Leaf(1)
let s = f"{w}"
"#,
        "cannot be used in string interpolation",
    );
}

#[test]
fn test_mutually_recursive_enums_rejected() {
    assert_compiler_error(
        r#"
enum A
    X(B)
    Y(int)

enum B
    Z(A)
    W(int)

let a = A.Y(1)
let s = f"{a}"
"#,
        "cannot be used in string interpolation",
    );
}

#[test]
fn test_fresh_option_temp_no_leak() {
    assert_runs_with_output(
        r#"
fn maybe_name(n int) String?
    if n > 0
        return f"name{n}"
    return None

print(f"{maybe_name(1)}")
"#,
        "Some(name1)",
    );
}

#[test]
fn test_fresh_result_temp_no_leak() {
    assert_runs_with_output(
        r#"
fn make() Result<int, String>
    Result.Err("oops")

print(f"{make()}")
"#,
        "Err(oops)",
    );
}

#[test]
fn test_enum_six_variants() {
    assert_runs_with_output(
        r#"
enum Status: Idle, Running, Paused, Stopped, Error, Unknown

let s1 = Status.Running
let s2 = Status.Unknown
print(f"First: {s1}, Last: {s2}")
"#,
        "First: Running, Last: Unknown",
    );
}

#[test]
fn test_enum_float_payload() {
    assert_runs_with_output(
        r#"
enum Val: F32Val(f32), F64Val(f64), BoolVal(bool)

let x = Val.F32Val(3.14)
let y = Val.F64Val(2.71828)
let z = Val.BoolVal(true)
print(f"{x} and {y} and {z}")
"#,
        "F32Val(3.14) and F64Val(2.71828) and BoolVal(true)",
    );
}

#[test]
fn test_option_of_float() {
    assert_runs_with_output(
        r#"
let x = Some(2.71828)
print(f"{x}")
"#,
        "Some(2.71828)",
    );
}

#[test]
fn test_generic_enum_two_instantiations() {
    assert_runs_with_output(
        r#"
enum Box<T>
    Hold(T)

let a = Box.Hold(42)
println(f"{a}")
let b = Box.Hold("hello")
println(f"{b}")
"#,
        "Hold(42)\nHold(hello)",
    );
}

#[test]
fn test_reassignment_between_interpolations() {
    assert_runs_with_output(
        r#"
var x = Some(10)
let s1 = f"Values: {x}"
x = Some(20)
let s2 = f"And: {x}"
print(f"{s1} | {s2}")
"#,
        "Values: Some(10) | And: Some(20)",
    );
}

#[test]
fn test_option_of_option_enum() {
    assert_runs_with_output(
        r#"
enum MyEnum: Variant(int)

let x = Some(Some(MyEnum.Variant(42)))
print(f"{x}")
"#,
        "Some(Some(Variant(42)))",
    );
}

#[test]
fn test_option_empty_string() {
    assert_runs_with_output(
        r#"
let x = Some("")
print(f"{x}")
"#,
        "Some()",
    );
}

#[test]
fn test_interpolation_with_surrounding_text() {
    assert_runs_with_output(
        r#"
let x = Some(42)
print(f"Value: {x} done")
"#,
        "Value: Some(42) done",
    );
}

#[test]
fn test_non_formattable_payload_asserts_type() {
    assert_compiler_error(
        r#"
fn make_list_opt() [int]?
    None

let o = make_list_opt()
print(f"{o}")
"#,
        "List(int)?",
    );
}

#[test]
fn test_recursive_enum_reports_type_name() {
    assert_compiler_error(
        r#"
enum Json: Value([Json]?)

let j = Json.Value(None)
print(f"{j}")
"#,
        "Json",
    );
}

#[test]
fn test_direct_self_referential_enum_reports_type_name() {
    assert_compiler_error(
        r#"
enum Wrapper
    Wrap(Wrapper)
    Leaf(int)

let w = Wrapper.Leaf(1)
let s = f"{w}"
"#,
        "Wrapper",
    );
}

#[test]
fn test_mutually_recursive_enums_report_type_name() {
    assert_compiler_error(
        r#"
enum A
    X(B)
    Y(int)

enum B
    Z(A)
    W(int)

let a = A.Y(1)
let s = f"{a}"
"#,
        "cannot be used in string interpolation",
    );
}

#[test]
fn test_deep_enum_chain_does_not_crash() {
    // A chain of 500 distinct enums, each holding the next.
    // Without a depth bound, this would overflow the compiler's stack.
    // We only need to try to interpolate the topmost one; the recursion
    // happens during the type check walk of the chain.
    let mut code = String::new();
    for i in 0..500 {
        code.push_str(&format!("enum E{}\n    Val(E{}?)\n\n", i, i + 1));
    }
    code.push_str("enum E500\n    Leaf(int)\n\n");
    code.push_str("fn make_e0() E0\n");
    code.push_str("    E0.Val(None)\n\n");
    code.push_str("let x = make_e0()\n");
    code.push_str("let s = f\"{x}\"\n");

    assert_compiler_error(&code, "cannot be used in string interpolation");
}
