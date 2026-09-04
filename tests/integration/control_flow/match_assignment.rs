// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::*;

#[test]
fn test_match_with_assignment_arms_should_compile() {
    let code = "
fn main()
    var i = 0
    var c = true
    match c
        true: i = i + 1
        false: c = false
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_match_with_compound_assignment_should_compile() {
    let code = "
fn main()
    var i = 0
    var c = true
    match c
        true: i += 1
        false: c = false
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_match_with_assignment_arms_reversed_should_compile() {
    let code = "
fn main()
    var i = 0
    var c = true
    match c
        true: c = false
        false: i = i + 1
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_match_assignment_arms_order_independent() {
    let code1 = "
fn main()
    var i = 0
    var c = true
    match c
        true: i = i + 1
        false: c = false
";
    let code2 = "
fn main()
    var i = 0
    var c = true
    match c
        true: c = false
        false: i = i + 1
";
    assert_type_checks(code1);
    assert_type_checks(code2);
}

#[test]
fn test_match_genuinely_mismatched_value_arms_should_fail() {
    let code = "
fn main()
    var c = true
    match c
        true: 1
        false: 'string'
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_conditional_with_assignment_arms_should_compile() {
    let code = "
fn main()
    var i = 0
    var c = true
    let x = if c: (i = i + 1) else: (c = false)
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_conditional_with_compound_assignment_should_compile() {
    let code = "
fn main()
    var i = 0
    var c = true
    let x = if c: (i += 1) else: (c = false)
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_conditional_with_assignment_arms_reversed_should_compile() {
    let code = "
fn main()
    var i = 0
    var c = true
    let x = if c: (c = false) else: (i = i + 1)
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_conditional_genuinely_mismatched_value_arms_should_fail() {
    let code = "
fn main()
    let x = if true: 1 else: 'string'
";
    assert_compiler_error(code, "Conditional branches must have the same type");
}

// Regression tests: mixed assignment + value arms must error (width mismatch)
#[test]
fn test_match_assignment_int_plus_string_value_errors() {
    let code = "
fn main()
    var a = 0
    match true
        true: a = 1
        false: 'string'
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_int_plus_string_value_reversed_errors() {
    let code = "
fn main()
    var a = 0
    match true
        true: 'string'
        false: a = 1
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_string_plus_int_value_errors() {
    let code = "
fn main()
    var s = 'hello'
    match true
        true: s = 'world'
        false: 42
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_string_plus_int_value_reversed_errors() {
    let code = "
fn main()
    var s = 'hello'
    match true
        true: 42
        false: s = 'world'
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_int_plus_void_runs() {
    let code = "
fn main()
    var a = 0
    match true
        true: a = 1
        false: println('x')
";
    assert_runs(code);
}

#[test]
fn test_match_assignment_plus_assignment_runs() {
    let code = "
fn main()
    var a = 0
    var b = 0
    match true
        true: a = 1
        false: b = 2
    println('done')
";
    assert_runs(code);
}

#[test]
fn test_conditional_assignment_int_plus_string_value_errors() {
    let code = "
fn main()
    var a = 0
    let x = if true: (a = 1) else: 'string'
";
    assert_compiler_error(code, "Conditional branches must have the same type");
}

#[test]
fn test_conditional_assignment_int_plus_string_value_reversed_errors() {
    let code = "
fn main()
    var a = 0
    let x = if true: 'string' else: (a = 1)
";
    assert_compiler_error(code, "Conditional branches must have the same type");
}

#[test]
fn test_conditional_assignment_int_plus_void_runs() {
    let code = "
fn main()
    var a = 0
    let x = if true: (a = 1) else: println('x')
    println('done')
";
    assert_runs(code);
}

// Managed-type crossing matrix: highest-value coverage gap
#[test]
fn test_match_assignment_scalar_lhs_plus_list_value_errors() {
    let code = "
use system.collections.list

fn main()
    var i = 0
    match true
        true: i = 1
        false: List([1, 2, 3])
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_scalar_lhs_plus_list_value_reversed_errors() {
    let code = "
use system.collections.list

fn main()
    var i = 0
    match true
        true: List([1, 2, 3])
        false: i = 1
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_scalar_lhs_plus_map_value_errors() {
    let code = "
use system.collections.map

fn main()
    var i = 0
    match true
        true: i = 1
        false: Map({'a': 1})
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_scalar_lhs_plus_map_value_reversed_errors() {
    let code = "
use system.collections.map

fn main()
    var i = 0
    match true
        true: Map({'a': 1})
        false: i = 1
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_scalar_lhs_plus_set_value_errors() {
    let code = "
use system.collections.set

fn main()
    var i = 0
    match true
        true: i = 1
        false: Set({1, 2, 3})
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_scalar_lhs_plus_set_value_reversed_errors() {
    let code = "
use system.collections.set

fn main()
    var i = 0
    match true
        true: Set({1, 2, 3})
        false: i = 1
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_string_lhs_plus_list_value_errors() {
    let code = "
use system.collections.list

fn main()
    var s = 'hello'
    match true
        true: s = 'world'
        false: List([1, 2, 3])
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_string_lhs_plus_list_value_reversed_errors() {
    let code = "
use system.collections.list

fn main()
    var s = 'hello'
    match true
        true: List([1, 2, 3])
        false: s = 'world'
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_conditional_assignment_scalar_lhs_plus_list_value_errors() {
    let code = "
use system.collections.list

fn main()
    var i = 0
    let x = if true: (i = 1) else: List([1, 2, 3])
";
    assert_compiler_error(code, "Conditional branches must have the same type");
}

#[test]
fn test_conditional_assignment_scalar_lhs_plus_list_value_reversed_errors() {
    let code = "
use system.collections.list

fn main()
    var i = 0
    let x = if true: List([1, 2, 3]) else: (i = 1)
";
    assert_compiler_error(code, "Conditional branches must have the same type");
}

#[test]
fn test_match_with_guard_and_value_arm_errors() {
    let code = "
fn main()
    var i = 0
    match 1
        x if x > 0: i = 1
        _: 'string'
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_with_multi_pattern_and_value_arm_errors() {
    let code = "
fn main()
    var i = 0
    match 1
        1 | 2: i = 1
        _: 'string'
";
    assert_compiler_error(code, "Match branch types mismatch");
}

#[test]
fn test_match_assignment_mutation_proves_execution() {
    let code = "
fn main()
    var x = 0
    match true
        true: x = 42
        false: x = 99
    println(f'{x}')
";
    assert_runs_with_output(code, "42");
}

#[test]
fn test_match_assignment_mutation_proves_execution_reversed() {
    let code = "
fn main()
    var x = 0
    match false
        true: x = 42
        false: x = 99
    println(f'{x}')
";
    assert_runs_with_output(code, "99");
}

#[test]
fn test_conditional_assignment_mutation_proves_execution() {
    let code = "
fn main()
    var x = 0
    if true:
        x = 42
    else:
        x = 99
    println(f'{x}')
";
    assert_runs_with_output(code, "42");
}

#[test]
fn test_conditional_assignment_mutation_proves_execution_reversed() {
    let code = "
fn main()
    var x = 0
    if false:
        x = 42
    else:
        x = 99
    println(f'{x}')
";
    assert_runs_with_output(code, "99");
}

#[test]
fn test_match_with_nested_match_and_value_arm_errors() {
    let code = "
fn main()
    var i = 0
    match 1
        1: match 2
            2: 0
            _: 1
        _: 'string'
";
    assert_compiler_error(code, "Match branch types mismatch");
}
