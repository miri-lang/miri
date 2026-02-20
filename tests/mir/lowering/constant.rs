// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn const_integer() {
    mir_snapshot_test(
        "fn main(): const x = 10",
        r#"
            let _0: void;
            let _1: int; // x

            bb0: {
                StorageLive(_1);
                _1 = const Integer(I8(10));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn const_typed_integer() {
    mir_snapshot_test(
        "fn main(): const x i32 = 42",
        r#"
            let _0: void;
            let _1: i32; // x

            bb0: {
                StorageLive(_1);
                _1 = const Integer(I8(42)) as i32;
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn const_with_expression() {
    mir_snapshot_test(
        "fn main(): const x = 1 + 2",
        r#"
            let _0: void;
            let _1: int; // x

            bb0: {
                StorageLive(_1);
                _1 = Add(const Integer(I8(1)), const Integer(I8(2)));
                StorageDead(_1);
                return;
            }
        "#,
    );
}
