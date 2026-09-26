// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_numeric_cast() {
    // The annotation types the literal, so the constant is already at the
    // declared width and no cast is lowered to reach it.
    mir_snapshot_test(
        r#"
fn main()
    let x i64 = 10
"#,
        r#"
            let _0: void;
            let _1: i64; // x

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
fn test_returned_literal_takes_the_return_width() {
    mir_snapshot_test(
        r#"
fn main() i64
    return 10
"#,
        r#"
            let _0: i64;

            bb0: {
                _0 = const Integer(I8(10));
                return;
            }
        "#,
    );
}

#[test]
fn test_assigned_literal_takes_the_binding_width() {
    mir_snapshot_test(
        r#"
fn main()
    var x i64 = 0
    x = 10
"#,
        r#"
            let _0: i64;
            let _1: i64; // x

            bb0: {
                StorageLive(_1);
                _1 = const Integer(I8(0));
                _1 = const Integer(I8(10));
                _0 = const Integer(I8(10));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_function_call_cast() {
    mir_snapshot_test(
        r#"
fn take_i64(x i64)
    return

fn main()
    take_i64(10)
"#,
        r#"
            let _0: void;
            let _1: void;
            let _2: void;

            bb0: {
                _1 = const Identifier("miri.take_i64")(const Integer(I8(10))) -> bb1;
            }

            bb1: {
                _2 = _1;
                return;
            }
        "#,
    );
}
