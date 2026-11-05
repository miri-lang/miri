// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A parameter's default is written in the callee and reads the callee's
//! names: the parameters declared before it hold the values this call passes
//! for them, and no binding of the caller is in sight.

use super::utils::*;

#[test]
fn a_default_reads_an_earlier_parameter() {
    assert_runs_with_output(
        r#"
fn f(k int, a int = k + 1) int
    return a

fn main()
    println(f"<{f(1)}> <{f(1, 7)}>")
"#,
        "<2> <7>",
    );
}

#[test]
fn a_default_reads_a_field_of_an_earlier_parameter() {
    assert_heap_guard_output(
        r#"
class Box
    var v int

fn f(b Box, a int = b.v) int
    return a

fn main()
    let c = Box(v: 1)
    println(f"<{f(c)}>")
"#,
        "<1>",
    );
}

#[test]
fn a_caller_binding_named_like_the_parameter_is_not_read() {
    assert_runs_with_output(
        r#"
fn f(k int, a int = k + 1) int
    return a

fn main()
    let k = 50
    println(f"{f(1)} {k}")
"#,
        "2 50",
    );
}

#[test]
fn a_default_reads_a_managed_parameter_the_call_also_passes() {
    assert_heap_guard_output(
        r#"
fn h(s String, t String = s) String
    return s + t

fn main()
    let x = "a" + "b"
    println(h(x))
    println(h("c" + "d"))
    println(h("lit"))
    println(x)
"#,
        "abab\ncdcd\nlitlit\nab",
    );
}

#[test]
fn a_default_reads_an_earlier_parameter_passed_as_a_field() {
    assert_heap_guard_output(
        r#"
class Holder
    var k int
    var s String

fn f(k int, a int = k * 10) int
    return a

fn g(s String, n int = s.length()) int
    return n

fn main()
    let h = Holder(k: 4, s: "abc" + "de")
    println(f"{f(h.k)} {g(h.s)} {h.s}")
"#,
        "40 5 abcde",
    );
}

#[test]
fn a_default_reads_an_earlier_default() {
    assert_runs_with_output(
        r#"
fn f(a int = 1, b int = a + 1) int
    return a * 100 + b

fn main()
    println(f"{f()} {f(5)} {f(5, 9)}")
"#,
        "102 506 509",
    );
}

#[test]
fn a_default_after_a_named_argument_reads_the_value_named() {
    assert_runs_with_output(
        r#"
fn f(a int, b int = a * 2, c int = b + 1) String
    return f"{a} {b} {c}"

fn main()
    println(f(c: 0, a: 3))
    println(f(b: 1, a: 3))
"#,
        "3 6 0\n3 1 2",
    );
}

#[test]
fn a_default_in_another_module_reads_its_earlier_parameter() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.lib.{f}\n",
                    "\n",
                    "fn main()\n",
                    "    let k = 50\n",
                    "    println(f\"{f(1)} {k}\")\n",
                ),
            ),
            ("lib.mi", "fn f(k int, a int = k + 1) int\n    return a\n"),
        ],
        "2 50",
    );
}

#[test]
fn a_generic_default_reads_an_earlier_parameter_at_each_instantiation() {
    assert_heap_guard_output(
        r#"
fn second<T>(x T, y T = x) T
    return y

fn main()
    let s = "a" + "b"
    println(f"{second(3)} {second(s)} {second(1.5, 2.5)} {s}")
"#,
        "3 ab 2.5 ab",
    );
}

#[test]
fn a_generic_call_receives_a_default_naming_no_parameter() {
    assert_runs_with_output(
        r#"
fn g<T>(x T, y int = 5) int
    return y

fn main()
    println(f"{g(3)} {g(1.5, 7)}")
"#,
        "5 7",
    );
}

#[test]
fn a_generic_default_written_at_the_type_parameter_builds_at_the_call_argument() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn count<T>(x T, xs [T] = List<T>()) int
    var all = xs
    all.push(x)
    return all.length()

fn main()
    let s = "a" + "b"
    println(f"{count(1)} {count(s)} {count(2, List([1, 2]))}")
"#,
        "1 1 3",
    );
}
