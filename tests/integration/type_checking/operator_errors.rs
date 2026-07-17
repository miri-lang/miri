// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Error-path tests for operator type checking.

use super::super::utils::assert_compiler_error;

/// Membership test with type mismatch: element not in List<T> where types differ.
#[test]
fn membership_type_mismatch_list_rejected() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    let result = "hello" in items
"#,
        "error",
    );
}

/// Membership test with type mismatch: element not in Set<T> where types differ.
#[test]
fn membership_type_mismatch_set_rejected() {
    assert_compiler_error(
        r#"
use system.collections.set

fn main()
    let items = Set([1, 2, 3])
    let result = "hello" in items
"#,
        "error",
    );
}

/// Membership test with type mismatch: element not in Map where key type differs.
#[test]
fn membership_type_mismatch_map_rejected() {
    assert_compiler_error(
        r#"
use system.collections.map

fn main()
    let items = Map({1: "a", 2: "b"})
    let result = "hello" in items
"#,
        "error",
    );
}

/// Membership test with type mismatch: element not in Range<T> where types differ.
#[test]
fn membership_type_mismatch_range_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let items = 1..10
    let result = "hello" in items
"#,
        "error",
    );
}

/// Membership test with type mismatch: element not in String where type differs.
#[test]
fn membership_type_mismatch_string_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let text = "hello"
    let result = 42 in text
"#,
        "error",
    );
}

/// Coalesce operator `??` with non-Option left operand is rejected.
#[test]
fn coalesce_non_option_left_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let x = 42 ?? 0
"#,
        "error",
    );
}

/// Bitwise AND on float operands is rejected.
#[test]
fn bitwise_and_on_float_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1.0 & 2.0
"#,
        "error",
    );
}

/// Bitwise OR on float operands is rejected.
#[test]
fn bitwise_or_on_float_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1.0 | 2.0
"#,
        "error",
    );
}

/// Bitwise XOR on float operands is rejected.
#[test]
fn bitwise_xor_on_float_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1.0 ^ 2.0
"#,
        "error",
    );
}

/// Negate operator on boolean operand is rejected.
#[test]
fn negate_bool_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let x = -true
"#,
        "error",
    );
}

/// Logical NOT operator on string operand is rejected.
#[test]
fn logical_not_string_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let x = !("hello")
"#,
        "error",
    );
}

/// Bitwise NOT operator on non-integer operand is rejected.
#[test]
fn bitwise_not_float_rejected() {
    assert_compiler_error(
        r#"
fn main()
    let y = ~1.0
"#,
        "error",
    );
}

/// Generic argument count mismatch: Map<int> is missing the value type.
#[test]
fn generic_arg_count_mismatch_map_rejected() {
    assert_compiler_error(
        r#"
use system.collections.map

fn main()
    let items = Map<int>()
"#,
        "error",
    );
}

/// Generic argument count mismatch: Pair<int> is missing the second type.
#[test]
fn generic_arg_count_mismatch_pair_rejected() {
    assert_compiler_error(
        r#"
class Pair<K, V>
    var key K
    var value V

fn main()
    let p = Pair<int>()
"#,
        "error",
    );
}
