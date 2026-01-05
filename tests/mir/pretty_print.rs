// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::ast::factory::*;
use miri::error::syntax::Span;
use miri::mir::{
    BasicBlockData, BinOp, Body, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};

#[test]
fn test_mir_pretty_print() {
    let int_type = type_int();

    let span = Span::default();
    let mut body = Body::new(2, span.clone(), false);

    // _0: Return value
    body.new_local(LocalDecl::new(int_type.clone(), span.clone()));
    // _1: arg0
    let mut arg0 = LocalDecl::new(int_type.clone(), span.clone());
    arg0.name = Some("arg0".to_string());
    body.new_local(arg0);
    // _2: arg1
    let mut arg1 = LocalDecl::new(int_type.clone(), span.clone());
    arg1.name = Some("arg1".to_string());
    body.new_local(arg1);
    // _3: temp
    let mut temp = LocalDecl::new(int_type.clone(), span.clone());
    temp.name = Some("temp".to_string());
    let l3 = body.new_local(temp);

    let mut bb0 = BasicBlockData::new(Some(Terminator::new(TerminatorKind::Return, span.clone())));

    // _3 = Add(_1, _2)
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(l3),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(Local(1)))),
                Box::new(Operand::Copy(Place::new(Local(2)))),
            ),
        ),
        span: span.clone(),
    });

    // _0 = _3
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(0)),
            Rvalue::Use(Operand::Copy(Place::new(l3))),
        ),
        span: span.clone(),
    });

    body.basic_blocks.push(bb0);

    let output = format!("{}", body);
    let expected = r#"
    let _0: Int;
    let _1: Int; // arg0
    let _2: Int; // arg1
    let _3: Int; // temp

    bb0: {
        _3 = Add(_1, _2);
        _0 = _3;
        return;
    }
"#;

    println!("Expected:\n{}", expected);
    println!("Actual:\n{}", output);

    assert_eq!(output.trim(), expected.trim());
}
