// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lower_code, mir_snapshot_contains_test, mir_snapshot_test};
use miri::ast::types::TypeKind;
use miri::mir::StatementKind;

#[test]
fn test_match_literal_patterns() {
    // Match generates a switchInt with targets for each literal pattern
    // Each branch assigns its result directly to the result local (_3)
    mir_snapshot_test(
        r#"
fn main()
    let x = 2
    match x
        1: "one"
        2: "two"
        _: "other"
"#,
        r#"
            let _0: String;
            let _1: int; // x
            let _2: int;
            let _3: String;
            let _4: int; // _

            bb0: {
                StorageLive(_1);
                _1 = const Integer(I8(2));
                _2 = _1;
                switchInt(_2) -> [1: bb2, 2: bb3, otherwise: bb4];
            }

            bb1: {
                _0 = _3;
                StorageDead(_1);
                return;
            }

            bb2: {
                _3 = const String("one");
                goto bb1;
            }

            bb3: {
                _3 = const String("two");
                goto bb1;
            }

            bb4: {
                StorageLive(_4);
                _4 = _2;
                _3 = const String("other");
                StorageDead(_4);
                goto bb1;
            }
        "#,
    );
}

#[test]
fn test_match_identifier_binding() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 42
    match x
        n: n
"#,
        &["// x", "// n"],
    );
}

#[test]
fn test_match_multiple_patterns() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let code = 200
    match code
        200 | 201 | 204: "success"
        404: "not found"
        _: "error"
"#,
        &[
            "// code",
            "switchInt",
            r#"String("success")"#,
            r#"String("not found")"#,
        ],
    );
}

#[test]
fn test_match_guard() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let num = 15
    match num
        x if x > 10: "large"
        x: "small"
"#,
        &["// num", "switchInt", "Gt("],
    );
}

#[test]
fn test_nested_match() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let a = 1
    let b = 2
    match a
        1: match b
            2: "inner"
            _: "other inner"
        _: "outer"
"#,
        &[
            "// a",
            "// b",
            "switchInt",
            r#"String("inner")"#,
            r#"String("outer")"#,
        ],
    );
}

#[test]
fn test_match_produces_basic_blocks() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 2
    match x
        1: "one"
        2: "two"
        _: "other"
"#,
        &["bb0:", "bb1:", "bb2:", "bb3:", "bb4:"],
    );
}

#[test]
fn test_match_enum_with_binding() {
    mir_snapshot_contains_test(
        r#"
enum Color: Red(String), Green(String), Blue(String)

fn main()
    let c = Color.Red('#ff0000')
    match c
        Color.Red(x): x
        Color.Green(x): x
        Color.Blue(x): x

"#,
        &["// c", "// x", "switchInt"],
    );
}

#[test]
fn test_match_many_literal_arms() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 5
    match x
        1: "one"
        2: "two"
        3: "three"
        4: "four"
        5: "five"
        6: "six"
        7: "seven"
        _: "other"
"#,
        &["// x", "switchInt", "1: bb", "7: bb"],
    );
}

#[test]
fn test_match_with_expression_in_arm() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 2
    match x
        1: 1 + 1
        2: 2 + 2
        _: 0
"#,
        &["// x", "switchInt", "Add("],
    );
}

#[test]
fn test_match_all_wildcards() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 42
    match x
        _: "any"
"#,
        &["// x", r#"String("any")"#],
    );
}

#[test]
fn test_match_deeply_nested() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let a = 1
    let b = 2
    let c = 3
    match a
        1: match b
            2: match c
                3: "deep"
                _: "not deep c"
            _: "not deep b"
        _: "not deep a"
"#,
        &["// a", "// b", "// c", r#"String("deep")"#],
    );
}

#[test]
fn test_match_result_used() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 1
    let result = match x
        1: 100
        _: 0
"#,
        &["// x", "// result", "const Integer(I8(100))"],
    );
}

// =============================================================================
// Regression tests for bind_pattern bug fixes
// =============================================================================

/// Regression test: tuple pattern destructuring must assign element types,
/// not the whole tuple type. Previously, each bound variable incorrectly
/// received the full tuple type (e.g., Tuple([int, string])) instead of the
/// individual element type (e.g., int or string).
#[test]
fn test_tuple_pattern_element_types() {
    let body = mir_lower_code(
        r#"
fn main()
    let t = (10, 20)
    match t
        (a, b): a
"#,
    );

    // Find locals named "a" and "b" and verify they have int type, not tuple type
    for decl in &body.local_decls {
        if decl.name.as_deref() == Some("a") || decl.name.as_deref() == Some("b") {
            assert!(
                !matches!(decl.ty.kind, TypeKind::Tuple(_)),
                "Tuple pattern variable '{}' should have element type, not tuple type. Got: {:?}",
                decl.name.as_deref().unwrap_or("?"),
                decl.ty.kind
            );
        }
    }
}

/// Regression test: enum variant pattern destructuring must use push_local
/// (which tracks scope) instead of directly inserting into variable_map.
/// This ensures StorageDead is emitted when the scope exits.
#[test]
fn test_enum_variant_binding_scope_tracking() {
    let body = mir_lower_code(
        r#"
enum Event: Click(int), Move(int)
fn main()
    let e = Event.Click(42)
    match e
        Event.Click(x): x
        Event.Move(y): y
"#,
    );

    // Verify that StorageLive is emitted for pattern-bound variables
    // (push_local emits StorageLive, push_temp does not when used with manual insert)
    let has_storage_live_for_binding = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::StorageLive(place) = &stmt.kind {
                let name = body.local_decls[place.local.0].name.as_deref();
                name == Some("x") || name == Some("y")
            } else {
                false
            }
        })
    });
    assert!(
        has_storage_live_for_binding,
        "Expected StorageLive for pattern-bound variables (x, y) to ensure proper scope tracking"
    );
}
