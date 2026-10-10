// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lower_code, mir_snapshot_test};
use miri::mir::symbol::Symbol;
use miri::mir::{Body, Operand, TerminatorKind};
use miri::type_checker::ModuleId;

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
                _1 = fn miri.foo() -> bb1;
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
                _1 = fn miri.add(const Integer(I8(1)), const Integer(I8(2))) -> bb1;
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
                _2 = fn miri.mul(const Integer(I8(2)), const Integer(I8(3))) -> bb1;
            }

            bb1: {
                _1 = fn miri.add(_2, const Integer(I8(4))) -> bb2;
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
                _1 = fn miri.do_something() -> bb1;
            }

            bb1: {
                _2 = _1;
                return;
            }
        "#,
    );
}

fn call_targets(body: &Body) -> Vec<&Operand> {
    body.basic_blocks
        .iter()
        .filter_map(|block| {
            let TerminatorKind::Call { func, .. } = &block.terminator.as_ref()?.kind else {
                return None;
            };
            Some(func)
        })
        .collect()
}

#[test]
fn test_call_to_declared_function_names_its_symbol() {
    let body = mir_lower_code(
        r#"
fn foo() int: 1
fn main()
    let x = foo()
"#,
    );
    let expected = Symbol::declared_function(&ModuleId::Program, "foo");
    let targets = call_targets(&body);
    assert!(
        targets
            .iter()
            .any(|func| matches!(func, Operand::Function(function) if function.symbol == expected)),
        "expected a call to `foo` through its symbol, got {targets:?}"
    );
}

#[test]
fn test_runtime_call_names_its_runtime_symbol() {
    let body = mir_lower_code("fn main(): let s = {1, 2, 3}");
    let expected = Symbol::runtime("miri_rt_set_add");
    let targets = call_targets(&body);
    assert!(
        targets
            .iter()
            .any(|func| matches!(func, Operand::Function(function) if function.symbol == expected)),
        "expected a call to the runtime's set insertion through its symbol, got {targets:?}"
    );
}
