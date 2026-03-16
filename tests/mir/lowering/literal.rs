// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_lower_integer_literal() {
    mir_snapshot_test(
        "fn main(): 42",
        r#"
            let _0: int;

            bb0: {
                _0 = const Integer(I8(42));
                return;
            }
        "#,
    );
}

#[test]
fn test_lower_negative_integer() {
    mir_snapshot_test(
        "fn main(): -10",
        r#"
            let _0: int;

            bb0: {
                _0 = Neg(const Integer(I8(10)));
                return;
            }
        "#,
    );
}

#[test]
fn test_lower_boolean_true() {
    mir_snapshot_test(
        "fn main(): true",
        r#"
            let _0: bool;

            bb0: {
                _0 = const Boolean(true);
                return;
            }
        "#,
    );
}

#[test]
fn test_lower_boolean_false() {
    mir_snapshot_test(
        "fn main(): false",
        r#"
            let _0: bool;

            bb0: {
                _0 = const Boolean(false);
                return;
            }
        "#,
    );
}

#[test]
fn test_lower_string_literal() {
    mir_snapshot_test(
        r#"fn main(): "hello""#,
        r#"
            let _0: String;

            bb0: {
                _0 = const String("hello");
                return;
            }
        "#,
    );
}

#[test]
fn test_lower_large_integer() {
    mir_snapshot_test(
        "fn main(): 1000",
        r#"
            let _0: int;

            bb0: {
                _0 = const Integer(I16(1000));
                return;
            }
        "#,
    );
}
