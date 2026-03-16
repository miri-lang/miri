// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::lower_function;
use miri::mir::{
    AggregateKind, BinOp, Body, Constant, GpuIntrinsic, Operand, Place, PlaceElem, Rvalue,
    StatementKind, StorageClass, TerminatorKind, UnOp,
};
use miri::pipeline::Pipeline;

pub fn mir_lower_code(source: &str) -> Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            if let AstStatementKind::FunctionDeclaration(func) = &stmt.node {
                func.name == "main"
            } else {
                false
            }
        })
        .or_else(|| {
            result
                .ast
                .body
                .iter()
                .find(|stmt| matches!(stmt.node, AstStatementKind::FunctionDeclaration(..)))
        })
        .expect("No function declaration found in source");

    lower_function(func_stmt, &result.type_checker, false, false).expect("Lowering failed").0
}

/// Normalize MIR output for comparison by trimming lines and removing empty lines.
fn normalize_mir_output(output: &str) -> String {
    output
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Snapshot test for MIR lowering.
/// Compares the actual MIR output against expected output.
/// Both are normalized (trimmed, empty lines removed) before comparison.
pub fn mir_snapshot_test(source: &str, expected_mir: &str) {
    let body = mir_lower_code(source);
    let actual = format!("{}", body);
    let actual_normalized = normalize_mir_output(&actual);
    let expected_normalized = normalize_mir_output(expected_mir);

    if actual_normalized != expected_normalized {
        panic!(
            "\n\nMIR snapshot mismatch!\n\n\
             === SOURCE ===\n{}\n\n\
             === EXPECTED ===\n{}\n\n\
             === ACTUAL ===\n{}\n",
            source.trim(),
            expected_normalized,
            actual_normalized
        );
    }
}

/// Snapshot test that only checks if the actual MIR contains the expected substrings.
/// Useful for partial validation when full MIR is too verbose.
pub fn mir_snapshot_contains_test(source: &str, expected_fragments: &[&str]) {
    let body = mir_lower_code(source);
    let actual = format!("{}", body);

    for fragment in expected_fragments {
        assert!(
            actual.contains(fragment),
            "\n\nMIR missing expected fragment!\n\n\
             === SOURCE ===\n{}\n\n\
             === MISSING FRAGMENT ===\n{}\n\n\
             === ACTUAL MIR ===\n{}\n",
            source.trim(),
            fragment,
            actual
        );
    }
}

pub fn expect_assignment(stmt: &miri::mir::Statement) -> (&Place, &Rvalue) {
    match &stmt.kind {
        StatementKind::Assign(place, rvalue) => (place, rvalue),
        _ => panic!("Expected Assign statement, got {:?}", stmt.kind),
    }
}

pub fn find_local_idx(body: &Body, name: &str) -> Option<usize> {
    body.local_decls
        .iter()
        .position(|d| d.name.as_deref() == Some(name))
}

pub fn has_local(body: &Body, name: &str) -> bool {
    body.local_decls
        .iter()
        .any(|d| d.name.as_deref() == Some(name))
}

pub fn count_locals_named(body: &Body, name: &str) -> usize {
    body.local_decls
        .iter()
        .filter(|d| d.name.as_deref() == Some(name))
        .count()
}

pub fn count_assignments(body: &Body, block_idx: usize) -> usize {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter(|s| matches!(&s.kind, StatementKind::Assign(..)))
        .count()
}

pub fn get_assignment_order(body: &Body, block_idx: usize) -> Vec<usize> {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter_map(|stmt| {
            if let StatementKind::Assign(place, _) = &stmt.kind {
                Some(place.local.0)
            } else {
                None
            }
        })
        .collect()
}

pub fn count_assignments_to(body: &Body, block_idx: usize, local_idx: usize) -> usize {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter(|s| {
            if let StatementKind::Assign(place, _) = &s.kind {
                place.local.0 == local_idx
            } else {
                false
            }
        })
        .count()
}

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

pub fn count_basic_blocks(body: &Body) -> usize {
    body.basic_blocks.len()
}

pub fn has_index_projection(body: &Body) -> bool {
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            if let StatementKind::Assign(place, _) = &stmt.kind {
                if place
                    .projection
                    .iter()
                    .any(|p| matches!(p, PlaceElem::Index(_)))
                {
                    return true;
                }
            }
            if let StatementKind::Assign(
                _,
                Rvalue::Use(Operand::Copy(place)) | Rvalue::Use(Operand::Move(place)),
            ) = &stmt.kind
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
    false
}

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

pub fn make_int_const(val: i32) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::Int, Span::default()),
        literal: Literal::Integer(IntegerLiteral::I32(val)),
    }))
}

pub fn make_string_const(val: &str) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::String, Span::default()),
        literal: Literal::String(val.to_string()),
    }))
}

pub fn mir_lowering_aggregate_test(source: &str, kind: AggregateKind, expected_count: usize) {
    let body = mir_lower_code(source);
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

pub fn mir_lowering_local_test(source: &str, name: &str) {
    let body = mir_lower_code(source);
    assert!(
        has_local(&body, name),
        "Expected local '{}' in MIR for source:\n{}",
        name,
        source
    );
}

pub fn mir_lowering_locals_test(source: &str, expected_locals: &[&str]) {
    let body = mir_lower_code(source);
    for name in expected_locals {
        assert!(has_local(&body, name), "Expected local '{}' to exist", name);
    }
}

pub fn mir_lowering_switch_int_test(source: &str, min_count: usize) {
    let body = mir_lower_code(source);
    let count = count_switch_int(&body);
    assert!(
        count >= min_count,
        "Expected at least {} SwitchInt terminators, got {} for source:\n{}",
        min_count,
        count,
        source
    );
}

pub fn mir_lowering_index_test(source: &str) {
    let body = mir_lower_code(source);
    assert!(
        has_index_projection(&body),
        "Expected Index projection in MIR for source:\n{}",
        source
    );
}

pub fn mir_lowering_tuple_aggregate_test(source: &str, expected_count: usize) {
    let body = mir_lower_code(source);
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

pub fn mir_lowering_binary_op_test(source: &str, expected_op: BinOp) {
    let body = mir_lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::Assign(_, Rvalue::BinaryOp(op, _, _)) = &stmt.kind {
                *op == expected_op
            } else {
                false
            }
        })
    });
    assert!(found, "Expected {:?} operation in MIR", expected_op);
}

pub fn mir_lowering_unary_op_test(source: &str, expected_op: UnOp) {
    let body = mir_lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::Assign(_, Rvalue::UnaryOp(op, _)) = &stmt.kind {
                *op == expected_op
            } else {
                false
            }
        })
    });
    assert!(found, "Expected {:?} operation in MIR", expected_op);
}

pub fn mir_lowering_terminator_test(source: &str, expected: TerminatorKind) {
    let body = mir_lower_code(source);
    let last_block = body.basic_blocks.last().expect("No basic blocks");
    let term = last_block.terminator.as_ref().expect("No terminator");
    assert!(
        std::mem::discriminant(&term.kind) == std::mem::discriminant(&expected),
        "Expected {:?} terminator, got {:?}",
        expected,
        term.kind
    );
}

pub fn mir_lowering_basic_blocks_test(source: &str, expected_count: usize) {
    let body = mir_lower_code(source);
    assert_eq!(
        body.basic_blocks.len(),
        expected_count,
        "Expected {} basic blocks for source:\n{}",
        expected_count,
        source
    );
}

pub fn mir_lowering_min_basic_blocks_test(source: &str, min_count: usize) {
    let body = mir_lower_code(source);
    assert!(
        body.basic_blocks.len() >= min_count,
        "Expected at least {} basic blocks, got {} for source:\n{}",
        min_count,
        body.basic_blocks.len(),
        source
    );
}

pub fn mir_lowering_call_count_test(source: &str, expected_count: usize) {
    let body = mir_lower_code(source);
    let calls_count = body
        .basic_blocks
        .iter()
        .filter(|bb| {
            if let Some(term) = &bb.terminator {
                matches!(term.kind, TerminatorKind::Call { .. })
            } else {
                false
            }
        })
        .count();
    assert_eq!(
        calls_count, expected_count,
        "Expected {} Call terminators for source:\n{}",
        expected_count, source
    );
}

pub fn mir_lowering_min_call_count_test(source: &str, min_count: usize) {
    let body = mir_lower_code(source);
    let calls_count = body
        .basic_blocks
        .iter()
        .filter(|bb| {
            if let Some(term) = &bb.terminator {
                matches!(term.kind, TerminatorKind::Call { .. })
            } else {
                false
            }
        })
        .count();
    assert!(
        calls_count >= min_count,
        "Expected at least {} Call terminators, got {} for source:\n{}",
        min_count,
        calls_count,
        source
    );
}

pub fn mir_lowering_gpu_flag_test(source: &str, expected_gpu: bool) {
    let body = mir_lower_code(source);
    assert_eq!(
        body.is_gpu(),
        expected_gpu,
        "Expected is_gpu() to be {} for source:\n{}",
        expected_gpu,
        source
    );
}

pub fn mir_lowering_gpu_intrinsic_test(source: &str, expected: GpuIntrinsic) {
    let body = mir_lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::Assign(_, Rvalue::GpuIntrinsic(intr)) = &stmt.kind {
                std::mem::discriminant(intr) == std::mem::discriminant(&expected)
                    && match (intr, &expected) {
                        (GpuIntrinsic::ThreadIdx(d1), GpuIntrinsic::ThreadIdx(d2)) => d1 == d2,
                        (GpuIntrinsic::BlockIdx(d1), GpuIntrinsic::BlockIdx(d2)) => d1 == d2,
                        (GpuIntrinsic::BlockDim(d1), GpuIntrinsic::BlockDim(d2)) => d1 == d2,
                        (GpuIntrinsic::GridDim(d1), GpuIntrinsic::GridDim(d2)) => d1 == d2,
                        _ => true,
                    }
            } else {
                false
            }
        })
    });
    assert!(
        found,
        "Expected {:?} GPU intrinsic in MIR for source:\n{}",
        expected, source
    );
}

pub fn mir_lowering_gpu_launch_test(source: &str) {
    let body = mir_lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        if let Some(terminator) = &bb.terminator {
            matches!(terminator.kind, TerminatorKind::GpuLaunch { .. })
        } else {
            false
        }
    });
    assert!(
        found,
        "Expected TerminatorKind::GpuLaunch for source:\n{}",
        source
    );
}

pub fn mir_lowering_storage_class_test(source: &str, var_name: &str, expected: StorageClass) {
    let body = mir_lower_code(source);
    let decl = body
        .local_decls
        .iter()
        .find(|d| d.name.as_deref() == Some(var_name));
    assert!(
        decl.is_some(),
        "Expected local '{}' for source:\n{}",
        var_name,
        source
    );
    assert_eq!(
        decl.unwrap().storage_class,
        expected,
        "Expected storage class {:?} for '{}' in source:\n{}",
        expected,
        var_name,
        source
    );
}

pub fn mir_lowering_literal_i8_test(source: &str, expected_value: i8) {
    let body = mir_lower_code(source);
    let bb0 = &body.basic_blocks[0];
    let found = bb0.statements.iter().any(|stmt| {
        if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &stmt.kind {
            matches!(c.literal, Literal::Integer(IntegerLiteral::I8(v)) if v == expected_value)
        } else {
            false
        }
    });
    assert!(
        found,
        "Expected integer literal {} in MIR for source:\n{}",
        expected_value, source
    );
}

pub fn mir_lowering_assignment_count_test(source: &str, var_name: &str, expected_count: usize) {
    let body = mir_lower_code(source);
    let idx = find_local_idx(&body, var_name);
    assert!(
        idx.is_some(),
        "Expected local '{}' for source:\n{}",
        var_name,
        source
    );
    let actual = count_assignments_to(&body, 0, idx.unwrap());
    assert_eq!(
        actual, expected_count,
        "Expected {} assignments to '{}' for source:\n{}",
        expected_count, var_name, source
    );
}

pub fn mir_lowering_switch_target_test(source: &str, block_idx: usize, expected_target: u128) {
    let body = mir_lower_code(source);
    let bb = &body.basic_blocks[block_idx];
    if let Some(term) = &bb.terminator {
        if let TerminatorKind::SwitchInt { targets, .. } = &term.kind {
            assert!(
                targets
                    .iter()
                    .any(|(val, _)| val.value() == expected_target),
                "Expected SwitchInt target {} in block {} for source:\n{}",
                expected_target,
                block_idx,
                source
            );
            return;
        }
    }
    panic!(
        "Expected SwitchInt terminator in block {} for source:\n{}",
        block_idx, source
    );
}

pub fn mir_lowering_goto_target_test(source: &str, block_idx: usize, expected_target: usize) {
    let body = mir_lower_code(source);
    let bb = &body.basic_blocks[block_idx];
    if let Some(term) = &bb.terminator {
        if let TerminatorKind::Goto { target } = &term.kind {
            assert_eq!(
                target.0, expected_target,
                "Expected Goto target {} in block {}, got {} for source:\n{}",
                expected_target, block_idx, target.0, source
            );
            return;
        }
    }
    panic!(
        "Expected Goto terminator in block {} for source:\n{}",
        block_idx, source
    );
}

pub fn mir_lowering_return_terminator_test(source: &str, block_idx: usize) {
    let body = mir_lower_code(source);
    let bb = &body.basic_blocks[block_idx];
    let term = bb.terminator.as_ref().expect("No terminator");
    assert!(
        matches!(term.kind, TerminatorKind::Return),
        "Expected Return terminator in block {} for source:\n{}",
        block_idx,
        source
    );
}

pub fn mir_lowering_pretty_print_contains_test(source: &str, expected_substring: &str) {
    let body = mir_lower_code(source);
    let output = format!("{}", body);
    assert!(
        output.contains(expected_substring),
        "Expected MIR output to contain '{}', got:\n{}",
        expected_substring,
        output
    );
}

pub fn mir_rvalue_display_starts_with_test(rvalue: &Rvalue, prefix: &str) {
    let display = format!("{}", rvalue);
    assert!(
        display.starts_with(prefix),
        "Expected display to start with '{}', got '{}'",
        prefix,
        display
    );
}

pub fn mir_rvalue_display_ends_with_test(rvalue: &Rvalue, suffix: &str) {
    let display = format!("{}", rvalue);
    assert!(
        display.ends_with(suffix),
        "Expected display to end with '{}', got '{}'",
        suffix,
        display
    );
}

pub fn mir_rvalue_display_contains_test(rvalue: &Rvalue, substring: &str) {
    let display = format!("{}", rvalue);
    assert!(
        display.contains(substring),
        "Expected display to contain '{}', got '{}'",
        substring,
        display
    );
}

pub fn mir_rvalue_equality_test(a: &Rvalue, b: &Rvalue) {
    assert_eq!(a, b, "Expected rvalues to be equal");
}

pub fn mir_lowering_min_assignments_test(source: &str, block_idx: usize, min_count: usize) {
    let body = mir_lower_code(source);
    let actual = count_assignments(&body, block_idx);
    assert!(
        actual >= min_count,
        "Expected at least {} assignments in block {}, got {} for source:\n{}",
        min_count,
        block_idx,
        actual,
        source
    );
}

pub fn mir_lowering_min_locals_test(source: &str, min_count: usize) {
    let body = mir_lower_code(source);
    assert!(
        body.local_decls.len() >= min_count,
        "Expected at least {} locals, got {} for source:\n{}",
        min_count,
        body.local_decls.len(),
        source
    );
}

pub fn mir_lowering_has_terminator_test(source: &str, block_idx: usize) {
    let body = mir_lower_code(source);
    assert!(
        body.basic_blocks[block_idx].terminator.is_some(),
        "Expected terminator in block {} for source:\n{}",
        block_idx,
        source
    );
}

pub fn mir_lowering_order_preserved_test(source: &str, var_names: &[&str]) {
    let body = mir_lower_code(source);
    let indices: Vec<_> = var_names
        .iter()
        .map(|name| find_local_idx(&body, name).unwrap_or_else(|| panic!("{} not found", name)))
        .collect();

    let order = get_assignment_order(&body, 0);
    let positions: Vec<_> = indices
        .iter()
        .map(|&idx| order.iter().position(|&x| x == idx).unwrap())
        .collect();

    for i in 0..positions.len() - 1 {
        assert!(
            positions[i] < positions[i + 1],
            "{} should come before {}",
            var_names[i],
            var_names[i + 1]
        );
    }
}

pub fn mir_class_compiles_test(source: &str) {
    let pipeline = Pipeline::new();
    pipeline.frontend(source).expect("Frontend should succeed");
}
