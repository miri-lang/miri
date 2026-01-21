// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_simple_call() {
    mir_snapshot_test(
        r#"
fn foo() int: 0
fn main()
    let x = foo()
"#,
        r#"
            let _0: void;
            let _1: int; // x

            bb0: {
                StorageLive(_1); 
                _1 = const Symbol("foo")() -> bb1;
            }

            bb1: {
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_call_with_arguments() {
    mir_snapshot_test(
        r#"
fn add(a int, b int) int: a + b
fn main()
    let x = add(1, 2)
"#,
        r#"
            let _0: void;
            let _1: int; // x

            bb0: {
                StorageLive(_1);
                _1 = const Symbol("add")(const Integer(I8(1)), const Integer(I8(2))) -> bb1;
            }

            bb1: {
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_nested_calls() {
    mir_snapshot_test(
        r#"
fn add(a int, b int) int: a + b
fn mul(a int, b int) int: a * b
fn main()
    let x = add(mul(2, 3), 4)
"#,
        r#"
            let _0: void;
            let _1: int; // x
            let _2: int;

            bb0: {
                StorageLive(_1);
                _2 = const Symbol("mul")(const Integer(I8(2)), const Integer(I8(3))) -> bb1;
            }

            bb1: {
                _1 = const Symbol("add")(_2, const Integer(I8(4))) -> bb2;
            }

            bb2: {
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_void_call_statement() {
    mir_snapshot_test(
        r#"
fn do_something()
    let x = 1
fn main()
    do_something()
"#,
        r#"
            let _0: void;
            let _1: void;
            let _2: void;

            bb0: {
                _1 = const Symbol("do_something")() -> bb1;
            }

            bb1: {
                _2 = _1;
                return;
            }
        "#,
    );
}
