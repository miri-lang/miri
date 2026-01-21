// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_snapshot_contains_test, mir_snapshot_test};

#[test]
fn test_tuple_literal() {
    mir_snapshot_test(
        "fn main(): let t = (1, 2, 3)",
        r#"
            let _0: void;
            let _1: tuple(int, int, int); // t

            bb0: {
                StorageLive(_1);
                _1 = (const Integer(I8(1)), const Integer(I8(2)), const Integer(I8(3)));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_empty_tuple() {
    mir_snapshot_test(
        "fn main(): let unit = ()",
        r#"
            let _0: void;
            let _1: tuple(); // unit

            bb0: {
                StorageLive(_1);
                _1 = ();
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_list_literal() {
    mir_snapshot_test(
        "fn main(): let l = [1, 2, 3]",
        r#"
            let _0: void;
            let _1: list(int); // l

            bb0: {
                StorageLive(_1);
                _1 = [const Integer(I8(1)), const Integer(I8(2)), const Integer(I8(3))];
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_set_literal() {
    mir_snapshot_contains_test(
        "fn main(): let s = {1, 2, 3}",
        &[
            "// s",
            "{const Integer(I8(1)), const Integer(I8(2)), const Integer(I8(3))}",
        ],
    );
}

#[test]
fn test_map_literal() {
    mir_snapshot_contains_test(
        r#"fn main(): let m = {"a": 1, "b": 2}"#,
        &[
            "// m",
            r#"{const String("a"): const Integer(I8(1)), const String("b"): const Integer(I8(2))}"#,
        ],
    );
}

#[test]
fn test_nested_tuple() {
    mir_snapshot_contains_test(
        "fn main(): let nested = ((1, 2), (3, 4))",
        &["// nested", "tuple(tuple(int, int), tuple(int, int))"],
    );
}

#[test]
fn test_list_index_access() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let l = [10, 20, 30]
    let x = l[1]
"#,
        &["// l", "// x", "_1[_3]"],
    );
}

#[test]
fn test_tuple_index_access() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let t = (1, 2, 3)
    let x = t[0]
"#,
        &["// t", "// x", "_1[_3]"],
    );
}

#[test]
fn test_empty_list() {
    // Note: empty list currently lowers as list(void); type annotation not propagated
    mir_snapshot_contains_test("fn main(): let l [int] = []", &["// l", "[]", "list(void)"]);
}

#[test]
fn test_nested_list() {
    mir_snapshot_contains_test(
        "fn main(): let l = [[1, 2], [3, 4]]",
        &["// l", "list(list("],
    );
}

#[test]
fn test_single_element_tuple() {
    mir_snapshot_test(
        "fn main(): let t = (42,)",
        r#"
            let _0: void;
            let _1: tuple(int); // t

            bb0: {
                StorageLive(_1);
                _1 = (const Integer(I8(42)));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_large_tuple() {
    mir_snapshot_contains_test(
        "fn main(): let t = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)",
        &[
            "// t",
            "tuple(int, int, int, int, int, int, int, int, int, int)",
        ],
    );
}

#[test]
fn test_large_list() {
    mir_snapshot_contains_test(
        "fn main(): let l = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]",
        &["// l", "list(int)"],
    );
}
