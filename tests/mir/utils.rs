// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::ast::statement::StatementKind;
use miri::mir::lowering::lower_function;
use miri::mir::Body;
use miri::pipeline::Pipeline;

pub fn lower_code(source: &str) -> Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    // Find the first function declaration
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| matches!(stmt.node, StatementKind::FunctionDeclaration(..)))
        .expect("No function declaration found in source");

    lower_function(func_stmt, &result.type_checker).expect("Lowering failed")
}

pub fn expect_assignment(stmt: &miri::mir::Statement) -> (&miri::mir::Place, &miri::mir::Rvalue) {
    match &stmt.kind {
        miri::mir::StatementKind::Assign(place, rvalue) => (place, rvalue),
        _ => panic!("Expected Assign statement, got {:?}", stmt.kind),
    }
}
