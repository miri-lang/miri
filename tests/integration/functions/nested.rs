// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn nested_function_calls_a_top_level_function() {
    assert_runs_with_output(
        r#"
use system.io

fn base(x int) int
    return x * 3

fn main()
    fn helper(n int) int
        return base(n)
    println(f"{helper(4)}")
"#,
        "12",
    );
}

#[test]
fn nested_function_calls_a_stdlib_method() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    fn shout(s String) String
        return s.to_upper()
    println(shout("quiet"))
"#,
        "QUIET",
    );
}

#[test]
fn nested_function_calls_another_nested_function() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    fn double(n int) int
        return n * 2
    fn double_plus_one(n int) int
        return double(n) + 1
    println(f"{double_plus_one(4)}")
"#,
        "9",
    );
}

#[test]
fn nested_function_calls_a_runtime_function() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_alloc(size int, align int) i64
runtime fn miri_free(ptr i64, size int, align int)

fn main()
    fn allocates(size int) int
        let ptr = miri_alloc(size, 8)
        if ptr == 0
            return 0
        miri_free(ptr, size, 8)
        return 1
    println(f"{allocates(64)}")
"#,
        "1",
    );
}

#[test]
fn nested_function_calls_a_math_intrinsic() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    fn hypotenuse(a float, b float) float
        return sqrt(a * a + b * b)
    println(f"{hypotenuse(3.0, 4.0)}")
"#,
        "5",
    );
}

#[test]
fn nested_function_that_captures_nothing_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    fn twice(n int) int
        return n * 2
    println(f"{twice(21)}")
"#,
        "42",
    );
}

#[test]
fn nested_function_reads_an_enclosing_local() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let offset = 5
    fn shifted(n int) int
        return n + offset
    println(f"{shifted(4)}")
"#,
        "9",
    );
}

#[test]
fn nested_function_reads_an_enclosing_managed_local() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let greeting = "hello"
    let names = List<String>()
    names.push("ada")
    fn greet(i int) String
        return f"{greeting}, {names[i]}"
    println(greet(0))
    println(greet(0))
"#,
        "hello, ada\nhello, ada",
    );
}

#[test]
fn nested_function_inside_a_non_main_function_calls_a_top_level_function() {
    assert_runs_with_output(
        r#"
use system.io

fn label(n int) String
    return f"item {n}"

fn render(count int) String
    fn item(i int) String
        return label(i * count)
    return f"{item(1)}, {item(2)}"

fn main()
    println(render(3))
"#,
        "item 3, item 6",
    );
}

#[test]
fn nested_functions_with_one_name_in_two_functions_stay_distinct() {
    assert_runs_with_output(
        r#"
use system.io

fn first() int
    fn helper(n int) int
        return n + 1
    return helper(10)

fn second() int
    fn helper(n int) int
        return n * 10
    return helper(10)

fn main()
    println(f"{first()} {second()}")
"#,
        "11 100",
    );
}

#[test]
fn nested_function_shadows_a_top_level_function_of_the_same_name() {
    assert_runs_with_output(
        r#"
use system.io

fn helper(_n int) int
    return 100

fn main()
    println(f"{helper(4)}")
    fn helper(n int) int
        return n * 2
    println(f"{helper(4)}")
"#,
        "100\n8",
    );
}

#[test]
fn nested_function_called_with_too_many_arguments_is_rejected() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    fn twice(n int) int
        return n * 2
    println(f"{twice(4, 5)}")
"#,
        "Too many positional arguments: expected 1, got 2",
    );
}

#[test]
fn nested_function_reads_a_managed_capture_on_every_call() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let n = 7
    let label = f"value {n}"
    let items = List<String>()
    items.push(f"item {n}")
    fn describe(i int) String
        let width = label.length()
        return f"{label} {items[i]} {width}"
    println(describe(0))
    println(describe(0))
    println(describe(0))
    println(label)
    println(items[0])
"#,
        "value 7 item 7 7\nvalue 7 item 7 7\nvalue 7 item 7 7\nvalue 7\nitem 7",
    );
}

#[test]
fn lambda_with_a_block_body_reads_a_managed_capture_on_every_call() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let n = 7
    let items = List<String>()
    items.push(f"item {n}")
    let first = fn(i int) String
        let count = items.length()
        return f"{items[i]} of {count}"
    println(first(0))
    println(first(0))
    println(first(0))
    println(items[0])
"#,
        "item 7 of 1\nitem 7 of 1\nitem 7 of 1\nitem 7",
    );
}

#[test]
fn nested_function_declared_after_a_managed_local_it_does_not_read() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let n = 3
    let unused = f"text {n}"
    println(unused)
    fn three() int
        return 3
    println(f"{three()}")
"#,
        "text 3\n3",
    );
}

#[test]
fn nested_function_assigning_a_captured_variable_leaves_the_outer_one_unchanged() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let n = 3
    var text = f"a{n}"
    fn replace() String
        let before = text
        text = f"b{n}"
        return f"{before}{text}"
    println(replace())
    println(replace())
    println(replace())
    println(text)
"#,
        "a3b3\na3b3\na3b3\na3",
    );
}

#[test]
fn nested_function_calls_itself() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    fn factorial(n int) int
        if n <= 1
            return 1
        return n * factorial(n - 1)
    println(f"{factorial(5)}")
"#,
        "120",
    );
}

#[test]
fn nested_function_calls_itself_not_a_top_level_function_it_shadows() {
    assert_runs_with_output(
        r#"
use system.io

fn factorial(_n int) int
    return 999

fn main()
    fn factorial(n int) int
        if n <= 1
            return 1
        return n * factorial(n - 1)
    println(f"{factorial(5)}")
"#,
        "120",
    );
}

#[test]
fn nested_function_recurses_over_a_captured_list() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let words = List<String>()
    words.push("a")
    words.push("b")
    words.push("c")
    fn join_from(i int) String
        if i >= words.length()
            return ""
        return f"{words[i]}{join_from(i + 1)}"
    println(join_from(0))
    println(join_from(1))
    println(words[2])
"#,
        "abc\nbc\nc",
    );
}

#[test]
fn nested_function_calls_itself_from_a_lambda_in_its_body() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    fn count(n int) int
        let step = fn(k int) int: count(k)
        if n == 0
            return 0
        return 1 + step(n - 1)
    println(f"{count(4)}")
"#,
        "4",
    );
}

#[test]
fn nested_function_passes_itself_as_a_value() {
    assert_runs_with_output(
        r#"
use system.io

fn apply(f fn(x int) int, v int) int
    return f(v)

fn main()
    fn factorial(n int) int
        if n <= 1
            return 1
        return n * apply(factorial, n - 1)
    println(f"{factorial(4)}")
"#,
        "24",
    );
}

#[test]
fn nested_function_returned_from_its_declaring_function_keeps_its_captures() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make() fn(n int) int
    let base = List<int>()
    base.push(10)
    fn down(n int) int
        if n == 0
            return base[0]
        let again = down
        return again(n - 1) + 1
    return down

fn main()
    let f = make()
    println(f"{f(3)}")
    println(f"{f(2)}")
"#,
        "13\n12",
    );
}

#[test]
fn nested_function_parameter_shadows_the_function_name() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    fn next(next int) int
        return next + 1
    println(f"{next(4)}")
"#,
        "5",
    );
}

#[test]
fn nested_function_assigning_a_captured_variable_it_never_reads() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let n = 3
    var text = f"a{n}"
    fn replace() int
        text = f"b{n}"
        return 1
    println(f"{replace()}")
    println(f"{replace()}")
    println(text)
"#,
        "1\n1\na3",
    );
}

#[test]
fn lambda_assigning_a_managed_capture_on_one_branch() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let n = 3
    var text = f"a{n}"
    let pick = fn(swap bool) String
        if swap
            text = f"b{n}"
        return text
    println(pick(true))
    println(pick(false))
    println(pick(true))
    println(text)
"#,
        "b3\na3\nb3\na3",
    );
}

#[test]
fn lambda_that_calls_its_enclosing_nested_function_outlives_it() {
    assert_runs_with_output(
        r#"
use system.io

fn make_stepper() fn(k int) int
    let label = "x"
    fn countdown(n int) fn(k int) int
        return fn(k int) int
            if k <= 0
                return label.length()
            let next = countdown(k - 1)
            return next(k - 1) + 1
    let stepper = countdown(3)
    return stepper

fn main()
    let stepper = make_stepper()
    println(f"{stepper(3)}")
"#,
        "4",
    );
}
