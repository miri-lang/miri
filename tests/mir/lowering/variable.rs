// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_lower_variable_declaration() {
    mir_snapshot_test(
        "fn main(): let x = 10",
        r#"
            let _0: void;
            let _1: int; // x

            bb0: {
                _1 = const Integer(I8(10));
                return;
            }
        "#,
    );
}

#[test]
fn test_variable_access_and_assignment() {
    // Note: The last assignment `x = 2` becomes both an assignment to x
    // and an implicit return value (the expression value is propagated to _0)
    mir_snapshot_test(
        "
fn main()
    var x = 1
    var y = x
    x = 2
",
        r#"
            let _0: int;
            let _1: int; // x
            let _2: int; // y

            bb0: {
                _1 = const Integer(I8(1));
                _2 = _1;
                _1 = const Integer(I8(2));
                _0 = const Integer(I8(2));
                return;
            }
        "#,
    );
}

#[test]
fn test_multiple_variables() {
    mir_snapshot_test(
        "
fn main()
    let a = 1
    let b = 2
    let c = 3
",
        r#"
            let _0: void;
            let _1: int; // a
            let _2: int; // b
            let _3: int; // c

            bb0: {
                _1 = const Integer(I8(1));
                _2 = const Integer(I8(2));
                _3 = const Integer(I8(3));
                return;
            }
        "#,
    );
}

#[test]
fn test_variable_with_expression() {
    // The temporary (_2) is created for the addition, then assigned to y (_3)
    mir_snapshot_test(
        "
fn main()
    let x = 5
    let y = x + 1
",
        r#"
            let _0: void;
            let _1: int; // x
            let _2: int;
            let _3: int; // y

            bb0: {
                _1 = const Integer(I8(5));
                _2 = Add(_1, const Integer(I8(1)));
                _3 = _2;
                return;
            }
        "#,
    );
}
