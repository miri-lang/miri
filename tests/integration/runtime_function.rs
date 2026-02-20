use crate::integration::utils::{assert_runs, assert_runs_with_output};

// ==================== Compilation with runtime declarations ====================

#[test]
fn runtime_function_declaration_compiles() {
    assert_runs(
        "
runtime fn miri_rt_noop()
fn main()
    let x = 42
",
    );
}

#[test]
fn runtime_function_with_params_compiles() {
    assert_runs(
        "
runtime fn miri_rt_string_len(ptr i64) int
fn main()
    let x = 42
",
    );
}

#[test]
fn multiple_runtime_functions_compile() {
    assert_runs(
        "
runtime fn miri_rt_alloc(size int) i64
runtime fn miri_rt_free(ptr i64)
fn main()
    let x = 42
",
    );
}

#[test]
fn explicit_core_runtime_compiles() {
    assert_runs(
        r#"
runtime "core" fn miri_rt_string_new() i64
fn main()
    let x = 42
"#,
    );
}

// ==================== Execution tests ====================

#[test]
fn test_runtime_string_len() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_len(s string) int
fn main()
    let s = "Hello, Miri!"
    println(miri_rt_string_len(s))
"#,
        "12",
    );
}

#[test]
fn test_runtime_string_empty() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_is_empty(s string) u8
fn main()
    let s1 = ""
    let s2 = "not empty"
    let result = if miri_rt_string_is_empty(s1) == 1 and miri_rt_string_is_empty(s2) == 0
        1
    else
        0
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_concat() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_concat(a string, b string) string
runtime fn miri_rt_string_equals(a string, b string) u8
runtime fn miri_rt_string_free(s string)

fn main()
    let a = "Hello, "
    let b = "World!"
    let combined = miri_rt_string_concat(a, b)
    let expected = "Hello, World!"

    let eq = miri_rt_string_equals(combined, expected)
    miri_rt_string_free(combined)

    let result = if eq == 1
        1
    else
        0
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_case_conversion() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_to_lower(s string) string
runtime fn miri_rt_string_to_upper(s string) string
runtime fn miri_rt_string_equals(a string, b string) u8
runtime fn miri_rt_string_free(s string)

fn main()
    let s = "Mixed CASE"
    let lower = miri_rt_string_to_lower(s)
    let upper = miri_rt_string_to_upper(s)

    let ok_lower = miri_rt_string_equals(lower, "mixed case")
    let ok_upper = miri_rt_string_equals(upper, "MIXED CASE")

    miri_rt_string_free(lower)
    miri_rt_string_free(upper)

    let result = if ok_lower == 1 and ok_upper == 1
        1
    else
        0
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_trim() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_trim(s string) string
runtime fn miri_rt_string_equals(a string, b string) u8
runtime fn miri_rt_string_free(s string)

fn main()
    let s = "  val  "
    let trimmed = miri_rt_string_trim(s)
    let eq = miri_rt_string_equals(trimmed, "val")
    miri_rt_string_free(trimmed)

    let result = if eq == 1
        1
    else
        0
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_alloc_free() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_alloc(size int, align int) i64
runtime fn miri_free(ptr i64, size int, align int)

fn main()
    let ptr = miri_alloc(1024, 8)
    let result = if ptr == 0
        0
    else
        miri_free(ptr, 1024, 8)
        1
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_realloc() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_alloc(size int, align int) i64
runtime fn miri_realloc(ptr i64, old_size int, align int, new_size int) i64
runtime fn miri_free(ptr i64, size int, align int)

fn main()
    let ptr = miri_alloc(64, 8)
    let new_ptr = miri_realloc(ptr, 64, 8, 128)

    let result = if new_ptr == 0
        0
    else
        miri_free(new_ptr, 128, 8)
        1
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_contains_starts_ends() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_contains(h string, n string) u8
runtime fn miri_rt_string_starts_with(s string, p string) u8
runtime fn miri_rt_string_ends_with(s string, suffix string) u8

fn main()
    let s = "foobarbaz"
    let c = miri_rt_string_contains(s, "bar")
    let st = miri_rt_string_starts_with(s, "foo")
    let e = miri_rt_string_ends_with(s, "baz")

    let result = if c == 1 and st == 1 and e == 1
        1
    else
        0
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_substring() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_string_substring(s string, start int, end int) string
runtime fn miri_rt_string_equals(a string, b string) u8
runtime fn miri_rt_string_free(s string)

fn main()
    let s = "hello world"
    let sub = miri_rt_string_substring(s, 0, 5)
    let eq = miri_rt_string_equals(sub, "hello")
    miri_rt_string_free(sub)

    let result = if eq == 1
        1
    else
        0
    println(result)
"#,
        "1",
    );
}

#[test]
fn test_runtime_io_smoke() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_println(s string)
fn main()
    miri_rt_println("IO Smoke Test")
    println(1)
"#,
        "1",
    );
}

#[test]
fn probe_string_len() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    runtime fn miri_rt_string_len(s string) int
    let s = "hello"
    println(miri_rt_string_len(s))
"#,
        "5",
    );
}
