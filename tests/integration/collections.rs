// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, interpreter_assert_returns};

// =============================================================================
// Tuple Tests
// =============================================================================

#[test]
fn test_tuple_creation() {
    assert_runs("let t = (1, 2, 3)");
}

#[test]
fn test_tuple_single_element() {
    assert_runs("let t = (42,)");
}

#[test]
fn test_tuple_mixed_types() {
    assert_runs(r#"let t = (1, "hello", true)"#);
}

#[test]
#[ignore = "MIR lowering: tuple field access projection not fully working"]
fn test_tuple_access() {
    interpreter_assert_returns(
        r#"
let t = (10, 20, 30)
t.0 + t.1
    "#,
        30,
    );
}

// =============================================================================
// List Tests
// =============================================================================

#[test]
fn test_list_creation() {
    assert_runs("let list = [1, 2, 3, 4, 5]");
}

#[test]
fn test_list_indexing() {
    interpreter_assert_returns(
        r#"
let list = [10, 20, 30]
list[1]
    "#,
        20,
    );
}

#[test]
fn test_list_index_assignment() {
    interpreter_assert_returns(
        r#"
var list = [10, 20, 30]
list[1] = 99
list[1]
    "#,
        99,
    );
}

// =============================================================================
// Map Tests
// =============================================================================

#[test]
fn test_map_creation() {
    assert_runs(r#"let m = {"a": 1, "b": 2}"#);
}

#[test]
fn test_map_single_entry() {
    assert_runs(r#"let m = {"key": 42}"#);
}

// =============================================================================
// Set Tests
// =============================================================================

#[test]
fn test_set_creation() {
    assert_runs("let s = {1, 2, 3}");
}
