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
use system.string

fn main()
    let s = "Hello, Miri!"
    println(f"{s.size()}")
"#,
        "12",
    );
}

#[test]
fn test_runtime_string_empty() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s1 = ""
    let s2 = "not empty"
    let a = if s1.is_empty()
        1
    else
        0
    let b = if s2.is_empty()
        0
    else
        1
    println(f"{a * b}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_concat() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let a = "Hello, "
    let b = "World!"
    let combined = a.concat(b)
    let expected = "Hello, World!"

    let result = if combined.equals(expected)
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_case_conversion() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "Mixed CASE"
    let lower = s.to_lower()
    let upper = s.to_upper()

    let result = if lower.equals("mixed case") and upper.equals("MIXED CASE")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_trim() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "  val  "
    let trimmed = s.trim()

    let result = if trimmed.equals("val")
        1
    else
        0
    println(f"{result}")
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
    println(f"{result}")
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
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_contains_starts_ends() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "foobarbaz"

    let result = if s.contains("bar") and s.starts_with("foo") and s.ends_with("baz")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_substring() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "hello world"
    let sub = s.substring(0, 5)

    let result = if sub.equals("hello")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_io_smoke() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_println(s String)
fn main()
    miri_rt_println("IO Smoke Test")
    println(f"{1}")
"#,
        "1",
    );
}

#[test]
fn probe_string_len() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "hello"
    println(f"{s.size()}")
"#,
        "5",
    );
}
