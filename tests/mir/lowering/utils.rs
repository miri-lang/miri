// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::lower_code;
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::{
    AggregateKind, BinOp, Body, Constant, Operand, PlaceElem, Rvalue, StatementKind, TerminatorKind,
};

/// Test that lowering produces an Aggregate of the expected kind with expected element count.
pub fn lowering_test_aggregate(source: &str, kind: AggregateKind, expected_count: usize) {
    let body = lower_code(source);
    let ops = find_aggregate_in_body(&body, &kind);
    assert!(
        ops.is_some(),
        "Expected {:?} aggregate in MIR for source:\n{}",
        kind,
        source
    );
    assert_eq!(
        ops.unwrap().len(),
        expected_count,
        "Expected {} elements in {:?} for source:\n{}",
        expected_count,
        kind,
        source
    );
}

/// Test that lowering creates a local variable with the given name.
pub fn lowering_test_has_local(source: &str, name: &str) {
    let body = lower_code(source);
    assert!(
        has_local(&body, name),
        "Expected local '{}' in MIR for source:\n{}",
        name,
        source
    );
}

/// Test that lowering creates at least the expected number of SwitchInt terminators.
pub fn lowering_test_switch_int(source: &str, min_count: usize) {
    let body = lower_code(source);
    let count = count_switch_int(&body);
    assert!(
        count >= min_count,
        "Expected at least {} SwitchInt terminators, got {} for source:\n{}",
        min_count,
        count,
        source
    );
}

/// Test that lowering produces an Index projection.
pub fn lowering_test_index_projection(source: &str) {
    let body = lower_code(source);
    assert!(
        has_index_projection(&body),
        "Expected Index projection in MIR for source:\n{}",
        source
    );
}

/// Find an aggregate of the given kind in the MIR body.
pub fn find_aggregate_in_body(body: &Body, expected_kind: &AggregateKind) -> Option<Vec<String>> {
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            if let StatementKind::Assign(_, Rvalue::Aggregate(kind, ops)) = &stmt.kind {
                if std::mem::discriminant(kind) == std::mem::discriminant(expected_kind) {
                    return Some(ops.iter().map(|op| format!("{}", op)).collect());
                }
            }
        }
    }
    None
}

/// Check if the body has any Index projection.
pub fn has_index_projection(body: &Body) -> bool {
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::Assign(place, _) => {
                    if place
                        .projection
                        .iter()
                        .any(|p| matches!(p, PlaceElem::Index(_)))
                    {
                        return true;
                    }
                }
                _ => {}
            }
            if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                if let Rvalue::Use(Operand::Copy(place)) | Rvalue::Use(Operand::Move(place)) =
                    rvalue
                {
                    if place
                        .projection
                        .iter()
                        .any(|p| matches!(p, PlaceElem::Index(_)))
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if a local variable with the given name exists.
pub fn has_local(body: &Body, name: &str) -> bool {
    body.local_decls
        .iter()
        .any(|d| d.name.as_deref() == Some(name))
}

/// Check if the body has any SwitchInt terminator.
pub fn has_switch_int(body: &Body) -> bool {
    body.basic_blocks.iter().any(|bb| {
        matches!(
            bb.terminator.as_ref().map(|t| &t.kind),
            Some(TerminatorKind::SwitchInt { .. })
        )
    })
}

/// Count the number of SwitchInt terminators.
pub fn count_switch_int(body: &Body) -> usize {
    body.basic_blocks
        .iter()
        .filter(|bb| {
            matches!(
                bb.terminator.as_ref().map(|t| &t.kind),
                Some(TerminatorKind::SwitchInt { .. })
            )
        })
        .count()
}

/// Count the number of basic blocks.
pub fn count_basic_blocks(body: &Body) -> usize {
    body.basic_blocks.len()
}

/// Create an integer constant operand.
pub fn make_int_const(val: i32) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::Int, Span::default()),
        literal: Literal::Integer(IntegerLiteral::I32(val)),
    }))
}

/// Create a string constant operand.
pub fn make_string_const(val: &str) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::String, Span::default()),
        literal: Literal::String(val.to_string()),
    }))
}

/// Test that lowering produces an Aggregate with specific element count (for enum variants).
pub fn lowering_test_aggregate_with_count(source: &str, expected_count: usize) {
    let body = lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Tuple, ops)) =
                &stmt.kind
            {
                ops.len() == expected_count
            } else {
                false
            }
        })
    });
    assert!(
        found,
        "Expected Tuple aggregate with {} elements for source:\n{}",
        expected_count, source
    );
}

pub fn lowering_test_binary_op(source: &str, expected_op: BinOp) {
    let body = lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let miri::mir::StatementKind::Assign(_, Rvalue::BinaryOp(op, _, _)) = &stmt.kind {
                *op == expected_op
            } else {
                false
            }
        })
    });
    assert!(found, "Expected {:?} operation in MIR", expected_op);
}
