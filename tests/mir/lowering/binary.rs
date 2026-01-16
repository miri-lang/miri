// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_add() {
    mir_snapshot_test(
        "fn main(): 1 + 2",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Add(const Integer(I8(1)), const Integer(I8(2)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_sub() {
    mir_snapshot_test(
        "fn main(): 5 - 3",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Sub(const Integer(I8(5)), const Integer(I8(3)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_mul() {
    mir_snapshot_test(
        "fn main(): 2 * 3",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Mul(const Integer(I8(2)), const Integer(I8(3)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_div() {
    mir_snapshot_test(
        "fn main(): 10 / 2",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Div(const Integer(I8(10)), const Integer(I8(2)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_mod() {
    mir_snapshot_test(
        "fn main(): 10 % 3",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Rem(const Integer(I8(10)), const Integer(I8(3)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_eq() {
    mir_snapshot_test(
        "fn main(): 1 == 1",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Eq(const Integer(I8(1)), const Integer(I8(1)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_ne() {
    mir_snapshot_test(
        "fn main(): 1 != 2",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Ne(const Integer(I8(1)), const Integer(I8(2)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_lt() {
    mir_snapshot_test(
        "fn main(): 1 < 2",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Lt(const Integer(I8(1)), const Integer(I8(2)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_le() {
    mir_snapshot_test(
        "fn main(): 1 <= 2",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Le(const Integer(I8(1)), const Integer(I8(2)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_gt() {
    mir_snapshot_test(
        "fn main(): 2 > 1",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Gt(const Integer(I8(2)), const Integer(I8(1)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_ge() {
    mir_snapshot_test(
        "fn main(): 2 >= 1",
        r#"
            let _0: boolean;
            let _1: boolean;

            bb0: {
                _1 = Ge(const Integer(I8(2)), const Integer(I8(1)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_bitwise_and() {
    mir_snapshot_test(
        "fn main(): 5 & 3",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = BitAnd(const Integer(I8(5)), const Integer(I8(3)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_deeply_nested_parentheses() {
    mir_snapshot_test(
        "fn main(): ((((1 + 2))))",
        r#"
            let _0: int;
            let _1: int;

            bb0: {
                _1 = Add(const Integer(I8(1)), const Integer(I8(2)));
                _0 = _1;
                return;
            }
        "#,
    );
}

#[test]
fn test_chained_additions() {
    // 1 + 2 + 3 + 4 + 5 produces multiple temporaries for intermediate results
    mir_snapshot_test(
        "fn main(): 1 + 2 + 3 + 4 + 5",
        r#"
            let _0: int;
            let _1: int;
            let _2: int;
            let _3: int;
            let _4: int;

            bb0: {
                _1 = Add(const Integer(I8(1)), const Integer(I8(2)));
                _2 = Add(_1, const Integer(I8(3)));
                _3 = Add(_2, const Integer(I8(4)));
                _4 = Add(_3, const Integer(I8(5)));
                _0 = _4;
                return;
            }
        "#,
    );
}

#[test]
fn test_chained_mixed_operations() {
    // 1 + 2 * 3 - 4 / 2 demonstrates precedence: 1 + (2*3) - (4/2) = 1 + 6 - 2 = 5
    mir_snapshot_test(
        "fn main(): 1 + 2 * 3 - 4 / 2",
        r#"
            let _0: int;
            let _1: int;
            let _2: int;
            let _3: int;
            let _4: int;

            bb0: {
                _1 = Mul(const Integer(I8(2)), const Integer(I8(3)));
                _2 = Add(const Integer(I8(1)), _1);
                _3 = Div(const Integer(I8(4)), const Integer(I8(2)));
                _4 = Sub(_2, _3);
                _0 = _4;
                return;
            }
        "#,
    );
}
