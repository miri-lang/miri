// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    mir_lowering_local_test, mir_lowering_min_basic_blocks_test, mir_lowering_switch_int_test,
};

#[test]
fn test_match_literal_patterns() {
    mir_lowering_switch_int_test(
        "
fn main()
    let x = 2
    match x
        1: \"one\"
        2: \"two\"
        _: \"other\"
",
        1,
    );
}

#[test]
fn test_match_identifier_binding() {
    mir_lowering_local_test(
        "
fn main()
    let x = 42
    match x
        n: n
",
        "n",
    );
}

#[test]
fn test_match_multiple_patterns() {
    mir_lowering_switch_int_test(
        "
fn main()
    let code = 200
    match code
        200 | 201 | 204: \"success\"
        404: \"not found\"
        _: \"error\"
",
        1,
    );
}

#[test]
fn test_match_guard() {
    mir_lowering_switch_int_test(
        "
fn main()
    let num = 15
    match num
        x if x > 10: \"large\"
        x: \"small\"
",
        2,
    );
}

#[test]
fn test_nested_match() {
    mir_lowering_switch_int_test(
        "
fn main()
    let a = 1
    let b = 2
    match a
        1: match b
            2: \"inner\"
            _: \"other inner\"
        _: \"outer\"
",
        2,
    );
}

#[test]
fn test_match_produces_basic_blocks() {
    mir_lowering_min_basic_blocks_test(
        "
fn main()
    let x = 2
    match x
        1: \"one\"
        2: \"two\"
        _: \"other\"
",
        5,
    );
}

#[test]
fn test_match_enum_with_binding() {
    mir_lowering_local_test(
        "
enum Color: Red(string), Green(string), Blue(string)

fn main()
    let c = Color.Red('#ff0000')
    match c
        Color.Red(x): x
        Color.Green(x): x
        Color.Blue(x): x

",
        "x",
    );
}

#[test]
fn test_match_many_literal_arms() {
    mir_lowering_switch_int_test(
        "
fn main()
    let x = 5
    match x
        1: \"one\"
        2: \"two\"
        3: \"three\"
        4: \"four\"
        5: \"five\"
        6: \"six\"
        7: \"seven\"
        _: \"other\"
",
        1,
    );
}

#[test]
fn test_match_with_expression_in_arm() {
    mir_lowering_switch_int_test(
        "
fn main()
    let x = 2
    match x
        1: 1 + 1
        2: 2 + 2
        _: 0
",
        1,
    );
}

#[test]
fn test_match_all_wildcards() {
    mir_lowering_local_test(
        "
fn main()
    let x = 42
    match x
        _: \"any\"
",
        "x",
    );
}

#[test]
fn test_match_deeply_nested() {
    mir_lowering_switch_int_test(
        "
fn main()
    let a = 1
    let b = 2
    let c = 3
    match a
        1: match b
            2: match c
                3: \"deep\"
                _: \"not deep c\"
            _: \"not deep b\"
        _: \"not deep a\"
",
        3,
    );
}

#[test]
fn test_match_result_used() {
    mir_lowering_local_test(
        "
fn main()
    let x = 1
    let result = match x
        1: 100
        _: 0
",
        "result",
    );
}
