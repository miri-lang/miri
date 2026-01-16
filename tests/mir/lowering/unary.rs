// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_neg() {
    mir_snapshot_test(
        "fn main(): -1",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Neg(const Integer(I8(1)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_not() {
    mir_snapshot_test(
        "fn main(): not true",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Not(const Boolean(true));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_double_negation() {
    // --1 creates two Neg operations, one nested inside the other
    mir_snapshot_test(
        "fn main(): --1",
        r#"
            let _0: int;
            let _1: int;
            let _2: int;

            bb0: {
                _1 = Neg(const Integer(I8(1)));
                _2 = Neg(_1);
                _0 = _2;
                return;
            }
        "#,
    );
}

#[test]
fn test_double_not() {
    mir_snapshot_test(
        "fn main(): not not true",
        r#"
            let _0: boolean;
            let _1: boolean;
            let _2: boolean;

            bb0: {
                _1 = Not(const Boolean(true));
                _2 = Not(_1);
                _0 = _2;
                return;
            }
        "#,
    );
}

#[test]
fn test_negation_with_parentheses() {
    // -(1 + 2) first computes the addition, then negates the result
    mir_snapshot_test(
        "fn main(): -(1 + 2)",
        r#"
            let _0: int;
            let _1: int;
            let _2: int;

            bb0: {
                _1 = Add(const Integer(I8(1)), const Integer(I8(2)));
                _2 = Neg(_1);
                _0 = _2;
                return;
            }
        "#,
    );
}

#[test]
fn test_not_with_comparison() {
    // not (1 < 2) first computes the comparison, then negates
    mir_snapshot_test(
        "fn main(): not (1 < 2)",
        r#"
            let _0: boolean;
            let _1: boolean;
            let _2: boolean;

            bb0: {
                _1 = Lt(const Integer(I8(1)), const Integer(I8(2)));
                _2 = Not(_1);
                _0 = _2;
                return;
            }
        "#,
    );
}
