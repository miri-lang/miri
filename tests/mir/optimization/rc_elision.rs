// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the RC elision optimization pass.
//!
//! Verifies that the pass removes redundant (IncRef, DecRef) pairs for values
//! that flow linearly through a function, and that programs still produce
//! correct results after elision.

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::optimization::rc_elision::{decref_in_range, elide_block, find_decref};
use miri::mir::optimization::{count_all_rc_ops, elide_rc, insert_rc};
use miri::mir::statement::StatementKind;
use miri::mir::terminator::TerminatorKind;
use miri::mir::{Local, LocalDecl, Operand, Place, Statement};
use miri::pipeline::Pipeline;
use std::collections::HashSet;

/// Lower `source` to MIR, run Perceus, run RC elision, and return all bodies.
fn get_elided_bodies(source: &str) -> Vec<(String, miri::mir::Body)> {
    let pipeline = Pipeline::new();
    pipeline
        .get_mir_bodies_with_rc(source)
        .expect("Pipeline should succeed")
}

/// Lower `source` to MIR, run Perceus only (no elision), return all bodies.
fn get_pre_elision_bodies(source: &str) -> Vec<(String, miri::mir::Body)> {
    use miri::ast::statement::StatementKind as AstStatementKind;
    use miri::mir::lowering::lower_function;

    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend should succeed");

    let mut bodies = Vec::new();
    for stmt in &result.ast.body {
        if let AstStatementKind::FunctionDeclaration(decl) = &stmt.node {
            let (mut body, lambdas) = lower_function(stmt, &result.type_checker, false, false)
                .expect("Lowering should succeed");
            insert_rc(&mut body);
            bodies.push((decl.name.clone(), body));
            for lambda in lambdas {
                bodies.push((lambda.name, lambda.body));
            }
        }
    }
    bodies
}

// ─── Benchmark: linear element access ────────────────────────────────────────

/// A function that accesses a list element linearly should have zero IncRef/DecRef
/// operations for the list parameter after elision.
///
/// Before elision, `element_at` creates a temporary `obj_local = Copy(items)` which
/// causes Perceus to emit `IncRef(items)` and `DecRef(obj_local)`. After elision both
/// are removed because `items` is not used after the copy.
#[test]
fn test_linear_element_access_has_zero_rc_ops() {
    let source = r#"
use system.collections.list

fn benchmark(items [int]) int:
    return items.element_at(0)
"#;
    let bodies = get_elided_bodies(source);
    let (_, body) = bodies
        .iter()
        .find(|(name, _)| name == "benchmark")
        .expect("benchmark function not found");

    let (incref, decref) = count_all_rc_ops(body);
    assert_eq!(
        incref, 0,
        "Expected 0 IncRef ops in linear element access, got {}",
        incref
    );
    assert_eq!(
        decref, 0,
        "Expected 0 DecRef ops in linear element access, got {}",
        decref
    );
}

/// Same function BEFORE elision should have non-zero RC ops (verifying elision actually fires).
#[test]
fn test_linear_element_access_has_rc_ops_before_elision() {
    let source = r#"
use system.collections.list

fn benchmark(items [int]) int:
    return items.element_at(0)
"#;
    let bodies = get_pre_elision_bodies(source);
    let (_, body) = bodies
        .iter()
        .find(|(name, _)| name == "benchmark")
        .expect("benchmark function not found");

    let (incref, decref) = count_all_rc_ops(body);
    assert!(
        incref > 0 || decref > 0,
        "Expected RC ops before elision, but found none (incref={}, decref={})",
        incref,
        decref
    );
}

// ─── Multiple linear accesses ─────────────────────────────────────────────────

/// Multiple element accesses on the same parameter — each creates a temporary
/// that should be elided independently.
#[test]
fn test_multiple_linear_accesses_elided() {
    let source = r#"
use system.collections.list

fn sum_first_two(items [int]) int:
    let x = items.element_at(0)
    let y = items.element_at(1)
    return x + y
"#;
    let bodies = get_elided_bodies(source);
    let (_, body) = bodies
        .iter()
        .find(|(name, _)| name == "sum_first_two")
        .expect("sum_first_two function not found");

    let (incref, decref) = count_all_rc_ops(body);
    assert_eq!(
        incref, 0,
        "Expected 0 IncRef ops after eliding multiple accesses, got {}",
        incref
    );
    assert_eq!(
        decref, 0,
        "Expected 0 DecRef ops after eliding multiple accesses, got {}",
        decref
    );
}

// ─── Scope-local elision: pairs in the same block ────────────────────────────

/// When elision removes all pairs, the body should have fewer RC ops than before.
/// This verifies that elision actually fires (it's not a no-op).
#[test]
fn test_elision_reduces_rc_op_count() {
    let source = r#"
use system.collections.list

fn benchmark(items [int]) int:
    return items.element_at(0)
"#;
    let pre_bodies = get_pre_elision_bodies(source);
    let post_bodies = get_elided_bodies(source);

    let (_, pre) = pre_bodies
        .iter()
        .find(|(name, _)| name == "benchmark")
        .expect("benchmark function not found (pre)");
    let (_, post) = post_bodies
        .iter()
        .find(|(name, _)| name == "benchmark")
        .expect("benchmark function not found (post)");

    let (pre_i, pre_d) = count_all_rc_ops(pre);
    let (post_i, post_d) = count_all_rc_ops(post);

    let pre_total = pre_i + pre_d;
    let post_total = post_i + post_d;
    assert!(
        post_total < pre_total,
        "Expected elision to reduce RC op count: pre={}, post={}",
        pre_total,
        post_total
    );
}

// ─── Verify MIR verification still passes after elision ─────────────────────

/// After RC elision, the MIR verifier must not report any violations.
/// This is the key soundness check: elision must not corrupt RC invariants.
#[test]
fn test_elision_does_not_break_mir_verification() {
    use miri::mir::verify::verify_body;

    let source = r#"
use system.collections.list

fn benchmark(items [int]) int:
    return items.element_at(0)
"#;
    let bodies = get_elided_bodies(source);
    for (name, body) in &bodies {
        let violations = verify_body(body);
        assert!(
            violations.is_empty(),
            "RC violations in '{}' after elision: {:?}",
            name,
            violations
        );
    }
}

// ─── Resource types must keep their DecRef ───────────────────────────────────

/// A type with a user-defined `fn drop(self)` destructor must NOT have its
/// IncRef/DecRef pair elided — the DecRef is what triggers the destructor.
///
/// Without the `has_drop` guard, the elision pass would remove the pair for
/// `copy = conn` and the destructor would not fire at `copy`'s StorageDead.
#[test]
fn test_resource_type_keeps_rc_ops() {
    // Conn has a destructor (fn drop), so its IncRef/DecRef pairs must be kept.
    let source = r#"
struct Conn
    handle int
    fn drop(self)
        return

fn use_conn(conn Conn) int:
    let copy = conn
    return copy.handle
"#;
    let bodies = get_elided_bodies(source);
    let (_, body) = bodies
        .iter()
        .find(|(name, _)| name == "use_conn")
        .expect("use_conn function not found");

    // Conn has a destructor — the pair must NOT be elided.
    // Both IncRef(conn_param) and DecRef(copy) should remain.
    let (incref, decref) = count_all_rc_ops(body);
    assert!(
        incref > 0,
        "Expected IncRef to be preserved for resource type, but found none"
    );
    assert!(
        decref > 0,
        "Expected DecRef to be preserved for resource type, but found none"
    );
}

// ─── Idempotency ─────────────────────────────────────────────────────────────

/// Running RC elision twice produces the same result as running it once.
#[test]
fn test_elision_is_idempotent() {
    use miri::ast::statement::StatementKind as AstStatementKind;
    use miri::mir::lowering::lower_function;

    let source = r#"
use system.collections.list

fn benchmark(items [int]) int:
    return items.element_at(0)
"#;
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend should succeed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            if let AstStatementKind::FunctionDeclaration(decl) = &stmt.node {
                decl.name == "benchmark"
            } else {
                false
            }
        })
        .expect("benchmark function not found");

    let (mut body, _) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("Lowering failed");

    insert_rc(&mut body);
    elide_rc(&mut body);
    let (inc1, dec1) = count_all_rc_ops(&body);

    // Run elision a second time — no change expected.
    elide_rc(&mut body);
    let (inc2, dec2) = count_all_rc_ops(&body);

    assert_eq!((inc1, dec1), (inc2, dec2), "RC elision is not idempotent");
}

fn span() -> Span {
    Span::new(0, 0)
}

fn make_incref(local: Local) -> Statement {
    Statement {
        kind: StatementKind::IncRef(Place {
            local,
            projection: vec![],
        }),
        span: span(),
    }
}

fn make_decref(local: Local) -> Statement {
    Statement {
        kind: StatementKind::DecRef(Place {
            local,
            projection: vec![],
        }),
        span: span(),
    }
}

fn make_assign(dest: Local, src: Local) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place {
                local: dest,
                projection: vec![],
            },
            miri::mir::Rvalue::Use(Operand::Copy(Place {
                local: src,
                projection: vec![],
            })),
        ),
        span: span(),
    }
}

fn make_nop() -> Statement {
    Statement {
        kind: StatementKind::StorageLive(Place {
            local: Local(99),
            projection: vec![],
        }),
        span: span(),
    }
}

fn make_local_decls_string(count: usize) -> Vec<miri::mir::LocalDecl> {
    let string_ty = Type::new(TypeKind::String, span());
    (0..count)
        .map(|_| LocalDecl::new(string_ty.clone(), span()))
        .collect()
}

// ─── decref_in_range ─────────────────────────────────────────────────────

#[test]
fn test_decref_in_range_finds_in_range() {
    let l = Local(0);
    let stmts = vec![make_decref(l)];
    assert!(decref_in_range(&stmts, l, 0, 1));
}

#[test]
fn test_decref_in_range_empty_range_returns_false() {
    let l = Local(0);
    let stmts = vec![make_decref(l)];
    assert!(!decref_in_range(&stmts, l, 0, 0));
}

#[test]
fn test_decref_in_range_target_after_end_returns_false() {
    let l = Local(0);
    let stmts = vec![make_nop(), make_nop(), make_decref(l)];
    // range [0, 2) does not cover position 2
    assert!(!decref_in_range(&stmts, l, 0, 2));
}

#[test]
fn test_decref_in_range_different_local_returns_false() {
    let l0 = Local(0);
    let l1 = Local(1);
    let stmts = vec![make_decref(l1)];
    assert!(!decref_in_range(&stmts, l0, 0, 1));
}

#[test]
fn test_decref_in_range_end_clamps_to_len() {
    let l = Local(0);
    let stmts = vec![make_decref(l)];
    // end=100 should clamp to stmts.len()=1 — the DecRef at [0] is in [0,1)
    assert!(decref_in_range(&stmts, l, 0, 100));
}

// ─── find_decref ─────────────────────────────────────────────────────────

#[test]
fn test_find_decref_finds_at_start() {
    let l = Local(0);
    let stmts = vec![make_decref(l)];
    assert_eq!(find_decref(&stmts, l, 0), Some(0));
}

#[test]
fn test_find_decref_finds_after_skip() {
    let l = Local(0);
    let stmts = vec![make_nop(), make_decref(l)];
    assert_eq!(find_decref(&stmts, l, 1), Some(1));
}

#[test]
fn test_find_decref_not_found_returns_none() {
    let l = Local(0);
    let stmts = vec![make_nop(), make_nop()];
    assert_eq!(find_decref(&stmts, l, 0), None);
}

#[test]
fn test_find_decref_skips_before_start() {
    let l = Local(0);
    let stmts = vec![make_decref(l), make_nop()];
    // start=1 skips the DecRef at 0
    assert_eq!(find_decref(&stmts, l, 1), None);
}

// ─── elide_block: decref_in_range safety gate ────────────────────────────

fn make_block(stmts: Vec<Statement>) -> miri::mir::block::BasicBlockData {
    miri::mir::block::BasicBlockData {
        statements: stmts,
        terminator: Some(miri::mir::Terminator {
            kind: TerminatorKind::Return,
            span: span(),
        }),
        is_cleanup: false,
    }
}

/// If DecRef(src) appears between the copy and DecRef(dest), elision must
/// NOT fire — src could be freed before dest's alias is cleaned up.
#[test]
fn test_elision_blocked_by_decref_in_range() {
    // Pattern:
    //   0: IncRef(src)
    //   1: dest = Copy(src)
    //   2: DecRef(src)     ← src freed before dest's DecRef
    //   3: DecRef(dest)
    let src = Local(1);
    let dest = Local(2);

    let mut block = make_block(vec![
        make_incref(src),
        make_assign(dest, src),
        make_decref(src),
        make_decref(dest),
    ]);
    let has_drop: HashSet<String> = HashSet::new();
    let local_decls = make_local_decls_string(3);

    let changed = elide_block(&mut block, &has_drop, &local_decls);

    assert!(
        !changed,
        "elide_block must not fire when DecRef(src) precedes DecRef(dest)"
    );
    assert_eq!(
        block.statements.len(),
        4,
        "all 4 statements must be preserved"
    );
}

/// Confirm that without the DecRef(src) in range, the pair IS elided.
#[test]
fn test_elision_fires_without_decref_in_range() {
    // Pattern:
    //   0: IncRef(src)
    //   1: dest = Copy(src)
    //   2: DecRef(dest)    ← no DecRef(src) in [2, 2) — safe
    let src = Local(1);
    let dest = Local(2);

    let mut block = make_block(vec![
        make_incref(src),
        make_assign(dest, src),
        make_decref(dest),
    ]);
    let has_drop: HashSet<String> = HashSet::new();
    let local_decls = make_local_decls_string(3);

    let changed = elide_block(&mut block, &has_drop, &local_decls);

    assert!(changed, "elide_block must fire for a clean linear pair");
    assert_eq!(
        block.statements.len(),
        1,
        "only the Assign should remain after elision"
    );
}
