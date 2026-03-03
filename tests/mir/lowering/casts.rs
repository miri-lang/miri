use crate::mir::utils::mir_snapshot_test;

#[test]
fn test_numeric_cast() {
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
                _1 = const Integer(I8(10)) as i64;
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_implicit_return_cast() {
    mir_snapshot_test(
        r#"
fn main() i64
    return 10
"#,
        r#"
            let _0: i64;

            bb0: {
                _0 = const Integer(I8(10)) as i64;
                return;
            }
        "#,
    );
}

#[test]
fn test_assignment_cast() {
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
                _1 = const Integer(I8(0)) as i64;
                _1 = const Integer(I8(10)) as i64;
                _0 = const Integer(I8(10)) as i64;
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
            let _1: i64;
            let _2: void;
            let _3: void;

            bb0: {
                _1 = const Integer(I8(10)) as i64;
                _2 = const Identifier("take_i64")(_1) -> bb1;
            }

            bb1: {
                _3 = _2;
                return;
            }
        "#,
    );
}
