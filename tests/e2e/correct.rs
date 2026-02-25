// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{miri_run, strip_ansi};

/// Run the given source, assert success, and check that stdout equals the expected output exactly.
fn assert_example_output(source: &str, expected_output: &str) {
    let result = miri_run(source);
    if !result.success {
        panic!(
            "Expected program to run successfully, but got errors:\n{}",
            result.output()
        );
    }
    let actual = strip_ansi(&result.stdout)
        .trim_end_matches('\n')
        .to_string();
    let expected = expected_output.trim_end_matches('\n');
    assert_eq!(
        actual, expected,
        "Program output did not match.\nExpected:\n{expected}\nActual:\n{actual}"
    );
}

#[test]
fn example_01_hello() {
    assert_example_output(
        include_str!("../examples/correct/01_hello.mi"),
        "Hello, World!",
    );
}

#[test]
fn example_02_arithmetic() {
    assert_example_output(
        include_str!("../examples/correct/02_arithmetic.mi"),
        "13\n7\n30\n3\n1",
    );
}

#[test]
fn example_03_variables() {
    assert_example_output(
        include_str!("../examples/correct/03_variables.mi"),
        "42\n15",
    );
}

#[test]
fn example_04_strings() {
    assert_example_output(
        include_str!("../examples/correct/04_strings.mi"),
        "Hello World\nHELLO\n11",
    );
}

#[test]
fn example_05_functions() {
    assert_example_output(
        include_str!("../examples/correct/05_functions.mi"),
        "7\nHello, Miri!",
    );
}

#[test]
fn example_06_recursion() {
    assert_example_output(
        include_str!("../examples/correct/06_recursion.mi"),
        "120\n55",
    );
}

#[test]
fn example_07_control_flow() {
    assert_example_output(
        include_str!("../examples/correct/07_control_flow.mi"),
        "1\n5\n3",
    );
}

#[test]
fn example_08_loops() {
    assert_example_output(
        include_str!("../examples/correct/08_loops.mi"),
        "15\n32\n120",
    );
}

#[test]
fn example_09_pattern_matching() {
    assert_example_output(
        include_str!("../examples/correct/09_pattern_matching.mi"),
        "zero\none\nnegative\nmany\nthe answer",
    );
}

#[test]
fn example_10_enums() {
    assert_example_output(
        include_str!("../examples/correct/10_enums.mi"),
        "north\n25\n16",
    );
}

#[test]
fn example_11_structs() {
    assert_example_output(
        include_str!("../examples/correct/11_structs.mi"),
        "3\n4\n25",
    );
}

#[test]
fn example_12_constants() {
    assert_example_output(
        include_str!("../examples/correct/12_constants.mi"),
        "3\n100\nHello",
    );
}

#[test]
fn example_13_fstrings() {
    assert_example_output(
        include_str!("../examples/correct/13_fstrings.mi"),
        "Hello, World!\nCount: 42\n84 is double 42\n5 + 3 = 8",
    );
}

#[test]
fn example_14_fizzbuzz() {
    assert_example_output(
        include_str!("../examples/correct/14_fizzbuzz.mi"),
        "1\n2\nFizz\n4\nBuzz\nFizz\n7\n8\nFizz\nBuzz\n11\nFizz\n13\n14\nFizzBuzz",
    );
}

#[test]
fn example_15_grade_calculator() {
    assert_example_output(
        include_str!("../examples/correct/15_grade_calculator.mi"),
        "A\nB\nC\nD\nF",
    );
}

#[test]
fn example_16_comparisons() {
    assert_example_output(
        include_str!("../examples/correct/16_comparisons.mi"),
        "true\nfalse\n5\n3\ntrue\ntrue\ntrue",
    );
}

#[test]
fn example_17_string_processing() {
    assert_example_output(
        include_str!("../examples/correct/17_string_processing.mi"),
        "HELLO, WORLD\ntrim me\ntrue\n12",
    );
}

#[test]
fn example_18_unless() {
    assert_example_output(
        include_str!("../examples/correct/18_unless.mi"),
        "small\ndone",
    );
}

#[test]
fn example_19_loops_advanced() {
    assert_example_output(
        include_str!("../examples/correct/19_loops_advanced.mi"),
        "5\n3\n4",
    );
}

#[test]
fn example_20_typed_params() {
    assert_example_output(
        include_str!("../examples/correct/20_typed_params.mi"),
        "42\n5.0\nfalse\nHELLO",
    );
}

#[test]
fn example_21_string_comparison() {
    assert_example_output(
        include_str!("../examples/correct/21_string_comparison.mi"),
        "equal\nnot equal\ntrue\nfalse",
    );
}

#[test]
fn example_22_float_ops() {
    assert_example_output(
        include_str!("../examples/correct/22_float_ops.mi"),
        "7.5\ntrue\nfalse\nx is 1.5",
    );
}
