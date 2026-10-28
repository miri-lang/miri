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
use miri::mir::symbol::Symbol;
use miri::mir::terminator::TerminatorKind;
use miri::mir::{Local, LocalDecl, Operand, Place, Statement};
use miri::pipeline::Pipeline;
use miri::type_checker::ModuleId;
use std::collections::{HashMap, HashSet};

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
            bodies.push((
                Symbol::declared_function(&ModuleId::Program, &decl.name).link_name(),
                body,
            ));
            for lambda in lambdas {
                bodies.push((lambda.symbol.link_name(), lambda.body));
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
        .find(|(name, _)| {
            *name == Symbol::function(&ModuleId::Program, "benchmark", &[]).link_name()
        })
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
        .find(|(name, _)| {
            *name == Symbol::function(&ModuleId::Program, "benchmark", &[]).link_name()
        })
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
        .find(|(name, _)| {
            *name == Symbol::function(&ModuleId::Program, "sum_first_two", &[]).link_name()
        })
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
        .find(|(name, _)| {
            *name == Symbol::function(&ModuleId::Program, "benchmark", &[]).link_name()
        })
        .expect("benchmark function not found (pre)");
    let (_, post) = post_bodies
        .iter()
        .find(|(name, _)| {
            *name == Symbol::function(&ModuleId::Program, "benchmark", &[]).link_name()
        })
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
///
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
class Conn
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
        .find(|(name, _)| {
            *name == Symbol::function(&ModuleId::Program, "use_conn", &[]).link_name()
        })
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

/// A local a borrowed local reads from keeps the retain that funds it: the
/// borrow relies on that reference, and eliding it would leave the local
/// sharing its source's value uncounted.
#[test]
fn test_elision_keeps_the_pair_funding_a_local_a_borrow_reads_from() {
    let src = Local(1);
    let dest = Local(2);
    let mut body = miri::mir::Body::new(0, span(), miri::mir::ExecutionModel::Cpu);
    for decl in make_local_decls_string(4) {
        body.new_local(decl);
    }
    body.basic_blocks.push(make_block(vec![
        make_incref(src),
        make_assign(dest, src),
        make_decref(dest),
    ]));
    body.borrowed_locals = HashMap::from([(Local(3), vec![dest])]);

    elide_rc(&mut body);

    assert_eq!(body.basic_blocks[0].statements.len(), 3, "the pair is kept");
}

/// A pair copying a borrowed local is kept: the copy's retain is its only
/// reference, since the borrow's value goes when its owner changes, with no
/// release of the borrow for the elision to see.
#[test]
fn test_elision_keeps_the_pair_copying_a_borrowed_local() {
    let src = Local(1);
    let dest = Local(2);
    let mut body = miri::mir::Body::new(0, span(), miri::mir::ExecutionModel::Cpu);
    for decl in make_local_decls_string(4) {
        body.new_local(decl);
    }
    body.basic_blocks.push(make_block(vec![
        make_incref(src),
        make_assign(dest, src),
        make_decref(dest),
    ]));
    body.borrowed_locals = HashMap::from([(src, vec![Local(3)])]);

    elide_rc(&mut body);

    assert_eq!(body.basic_blocks[0].statements.len(), 3, "the pair is kept");
}

mod owned_transfers {
    //! A retain funding an argument the callee takes over, cancelled against
    //! the caller's release of the same local right after the call.

    use miri::ast::literal::Literal;
    use miri::ast::types::{Type, TypeKind};
    use miri::error::syntax::Span;
    use miri::mir::block::{BasicBlock, BasicBlockData};
    use miri::mir::optimization::elide_rc;
    use miri::mir::symbol::Symbol;
    use miri::mir::{
        ArgMode, Body, Constant, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
        Statement, StatementKind, Terminator, TerminatorKind,
    };
    use miri::type_checker::ModuleId;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn place(local: usize) -> Place {
        Place::new(Local(local))
    }

    fn stmt(kind: StatementKind) -> Statement {
        Statement { kind, span: span() }
    }

    fn incref(local: usize) -> Statement {
        stmt(StatementKind::IncRef(place(local)))
    }

    fn decref(local: usize) -> Statement {
        stmt(StatementKind::DecRef(place(local)))
    }

    fn storage_dead(local: usize) -> Statement {
        stmt(StatementKind::StorageDead(place(local)))
    }

    fn fresh_string() -> Rvalue {
        Rvalue::Use(Operand::Constant(Box::new(Constant {
            ty: Type::new(TypeKind::String, span()),
            literal: Literal::String("x".to_string()),
            span: span(),
        })))
    }

    fn terminated(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
        BasicBlockData {
            statements,
            terminator: Some(Terminator { kind, span: span() }),
            is_cleanup: false,
        }
    }

    /// `_2 = callee(args)` with `modes`, continuing at bb1.
    fn call(args: Vec<Operand>, modes: Vec<ArgMode>) -> TerminatorKind {
        TerminatorKind::Call {
            func: Operand::function(
                Symbol::declared_function(&ModuleId::Program, "callee"),
                span(),
            ),
            args,
            arg_modes: modes,
            arg_handles: Vec::new(),
            destination: place(2),
            target: Some(BasicBlock(1)),
        }
    }

    /// Locals: 0 the return slot, 1 the argument `L` of type `argument`, 2 the
    /// call's unmanaged result, 3 a second string.
    fn caller(argument: Type, blocks: Vec<BasicBlockData>) -> Body {
        let mut body = Body::new(0, span(), ExecutionModel::Cpu);
        let void = Type::new(TypeKind::Void, span());
        let string = Type::new(TypeKind::String, span());
        for ty in [void.clone(), argument, void, string] {
            body.new_local(LocalDecl::new(ty, span()));
        }
        body.basic_blocks = blocks;
        body
    }

    fn string_ty() -> Type {
        Type::new(TypeKind::String, span())
    }

    /// `L` built, funded and handed over at the call, then released and dead.
    fn handing_over(after_call: Vec<Statement>) -> Vec<BasicBlockData> {
        vec![
            terminated(
                vec![
                    stmt(StatementKind::Assign(place(1), fresh_string())),
                    incref(1),
                ],
                call(vec![Operand::Copy(place(1))], vec![ArgMode::Owned]),
            ),
            terminated(after_call, TerminatorKind::Return),
        ]
    }

    fn rc_ops_on_l(body: &Body) -> usize {
        body.basic_blocks
            .iter()
            .flat_map(|block| &block.statements)
            .filter(|statement| {
                matches!(&statement.kind,
                    StatementKind::IncRef(p) | StatementKind::DecRef(p) if p.local == Local(1))
            })
            .count()
    }

    #[test]
    fn a_dying_owned_argument_hands_its_own_reference_over() {
        let mut dying = caller(string_ty(), handing_over(vec![decref(1), storage_dead(1)]));
        elide_rc(&mut dying);
        assert_eq!(rc_ops_on_l(&dying), 0, "{dying}");

        let mut rebound = caller(
            string_ty(),
            handing_over(vec![
                decref(1),
                stmt(StatementKind::Reassign(place(1), fresh_string())),
            ]),
        );
        elide_rc(&mut rebound);
        assert_eq!(rc_ops_on_l(&rebound), 0, "{rebound}");
    }

    #[test]
    fn an_argument_passed_twice_keeps_its_retain() {
        let mut blocks = handing_over(vec![decref(1), storage_dead(1)]);
        blocks[0].terminator = Some(Terminator {
            kind: call(
                vec![Operand::Copy(place(1)), Operand::Copy(place(1))],
                vec![ArgMode::Owned, ArgMode::Borrowed],
            ),
            span: span(),
        });
        let mut body = caller(string_ty(), blocks);
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_l(&body), 2, "{body}");
    }

    #[test]
    fn a_continuation_reached_from_elsewhere_keeps_the_retain() {
        let mut blocks = handing_over(vec![decref(1), storage_dead(1)]);
        blocks.push(terminated(
            Vec::new(),
            TerminatorKind::Goto {
                target: BasicBlock(1),
            },
        ));
        let mut body = caller(string_ty(), blocks);
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_l(&body), 2, "{body}");
    }

    #[test]
    fn an_argument_read_after_the_call_keeps_the_retain() {
        let read = stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::Copy(place(1))),
        ));
        let mut body = caller(
            string_ty(),
            handing_over(vec![read, decref(1), storage_dead(1)]),
        );
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_l(&body), 2, "{body}");
    }

    /// The entry block is entered by the edge into the body as well as by the
    /// call, so a release found there may run with no call before it.
    #[test]
    fn a_call_continuing_at_the_entry_block_keeps_the_retain() {
        let mut call_to_entry = call(vec![Operand::Copy(place(1))], vec![ArgMode::Owned]);
        if let TerminatorKind::Call { target, .. } = &mut call_to_entry {
            *target = Some(BasicBlock(0));
        }
        let blocks = vec![
            terminated(
                vec![decref(1), storage_dead(1)],
                TerminatorKind::Goto {
                    target: BasicBlock(1),
                },
            ),
            terminated(
                vec![
                    stmt(StatementKind::Assign(place(1), fresh_string())),
                    incref(1),
                ],
                call_to_entry,
            ),
        ];
        let mut body = caller(string_ty(), blocks);
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_l(&body), 2, "{body}");
    }

    #[test]
    fn an_argument_with_a_drop_hook_keeps_the_retain() {
        let resource = Type::new(TypeKind::Custom("Res".to_string(), None), span());
        let mut body = caller(resource, handing_over(vec![decref(1), storage_dead(1)]));
        body.has_drop_types.insert("Res".to_string());
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_l(&body), 2, "{body}");
    }
}

mod dying_sources {
    //! A retain funding a copy of a local that dies right after it, cancelled
    //! against that local's release: the copy takes the local's own reference.

    use miri::ast::literal::Literal;
    use miri::ast::types::{Type, TypeKind};
    use miri::error::syntax::Span;
    use miri::mir::block::BasicBlockData;
    use miri::mir::optimization::elide_rc;
    use miri::mir::verify::verify_body;
    use miri::mir::{
        AggregateKind, Body, Constant, ExecutionModel, Local, LocalDecl, Operand, Place, PlaceElem,
        Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
    };
    use std::collections::HashMap;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn place(local: usize) -> Place {
        Place::new(Local(local))
    }

    fn stmt(kind: StatementKind) -> Statement {
        Statement { kind, span: span() }
    }

    fn incref(local: usize) -> Statement {
        stmt(StatementKind::IncRef(place(local)))
    }

    fn decref(local: usize) -> Statement {
        stmt(StatementKind::DecRef(place(local)))
    }

    fn storage_dead(local: usize) -> Statement {
        stmt(StatementKind::StorageDead(place(local)))
    }

    fn string_ty() -> Type {
        Type::new(TypeKind::String, span())
    }

    fn optional_string_ty() -> Type {
        Type::new(TypeKind::Option(Box::new(string_ty())), span())
    }

    fn fresh(local: usize) -> Statement {
        stmt(StatementKind::Assign(
            place(local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                ty: string_ty(),
                literal: Literal::String("x".to_string()),
                span: span(),
            }))),
        ))
    }

    /// `_2 = Some(_1)`.
    fn wrap_source() -> Statement {
        stmt(StatementKind::Assign(
            place(2),
            Rvalue::Aggregate(
                AggregateKind::Option(optional_string_ty()),
                vec![Operand::Copy(place(1))],
            ),
        ))
    }

    /// `_3 = _1`, or `reassign _3 = _1` when `is_reassign`.
    fn copy_source(is_reassign: bool) -> Statement {
        let rvalue = Rvalue::Use(Operand::Copy(place(1)));
        stmt(if is_reassign {
            StatementKind::Reassign(place(3), rvalue)
        } else {
            StatementKind::Assign(place(3), rvalue)
        })
    }

    /// Locals: 0 the return slot, 1 the source `x` of type `source`, 2 an
    /// optional string, 3 a string; one block holding `statements`.
    fn body_of(source: Type, statements: Vec<Statement>) -> Body {
        let mut body = Body::new(0, span(), ExecutionModel::Cpu);
        let void = Type::new(TypeKind::Void, span());
        for ty in [void, source, optional_string_ty(), string_ty()] {
            body.new_local(LocalDecl::new(ty, span()));
        }
        body.basic_blocks = vec![BasicBlockData {
            statements,
            terminator: Some(Terminator {
                kind: TerminatorKind::Return,
                span: span(),
            }),
            is_cleanup: false,
        }];
        body
    }

    fn rc_ops_on_x(body: &Body) -> usize {
        body.basic_blocks
            .iter()
            .flat_map(|block| &block.statements)
            .filter(|statement| {
                matches!(&statement.kind,
                    StatementKind::IncRef(p) | StatementKind::DecRef(p) if p.local == Local(1))
            })
            .count()
    }

    /// `x` built, retained for `transfer`, released and dead, with `between`
    /// after the transfer and before the release; the destination of the
    /// transfer is released at the end.
    fn dying(transfer: Statement, between: Vec<Statement>, dest: usize) -> Vec<Statement> {
        let mut statements = vec![fresh(1), incref(1), transfer];
        statements.extend(between);
        statements.extend([decref(1), storage_dead(1), decref(dest), storage_dead(dest)]);
        statements
    }

    fn elided(statements: Vec<Statement>) -> Body {
        let mut body = body_of(string_ty(), statements);
        elide_rc(&mut body);
        body
    }

    fn assert_moved(body: &Body) {
        assert_eq!(rc_ops_on_x(body), 0, "{body}");
        assert!(
            verify_body(body).is_empty(),
            "{:?}\n{body}",
            verify_body(body)
        );
    }

    #[test]
    fn a_dying_local_wrapped_in_an_optional_hands_its_reference_over() {
        let body = elided(dying(wrap_source(), vec![storage_dead(3)], 2));
        assert_moved(&body);
        let StatementKind::Assign(_, Rvalue::Aggregate(_, operands)) =
            &body.basic_blocks[0].statements[1].kind
        else {
            panic!("the aggregate stays second: {body}");
        };
        assert_eq!(operands, &vec![Operand::Move(place(1))], "{body}");
    }

    #[test]
    fn a_dying_local_copied_or_rebound_hands_its_reference_over() {
        for is_reassign in [false, true] {
            let body = elided(dying(copy_source(is_reassign), Vec::new(), 3));
            assert_moved(&body);
        }
    }

    /// The return slot is never released in the body, so only the source's
    /// own release can pay for the retain.
    #[test]
    fn a_dying_local_returned_hands_its_reference_over() {
        let returned = stmt(StatementKind::Assign(
            place(0),
            Rvalue::Use(Operand::Copy(place(1))),
        ));
        let statements = vec![
            fresh(1),
            incref(1),
            returned,
            storage_dead(2),
            decref(1),
            storage_dead(1),
        ];
        let mut body = body_of(string_ty(), statements);
        body.local_decls[0] = LocalDecl::new(string_ty(), span());
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_x(&body), 0, "{body}");
    }

    #[test]
    fn a_source_read_after_the_copy_keeps_the_pair() {
        let read = stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::Copy(place(1))),
        ));
        let body = elided(dying(wrap_source(), vec![read], 2));
        assert_eq!(rc_ops_on_x(&body), 2, "{body}");
    }

    #[test]
    fn a_source_copied_twice_keeps_one_pair() {
        let body = elided(vec![
            fresh(1),
            incref(1),
            copy_source(false),
            incref(1),
            wrap_source(),
            decref(1),
            storage_dead(1),
            decref(2),
            storage_dead(2),
            decref(3),
            storage_dead(3),
        ]);
        assert!(rc_ops_on_x(&body) >= 1, "{body}");
        assert!(
            verify_body(&body).is_empty(),
            "{:?}\n{body}",
            verify_body(&body)
        );
    }

    #[test]
    fn a_source_living_on_after_its_release_keeps_the_pair() {
        let rebuilt = fresh(1);
        let read = stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::Copy(place(1))),
        ));
        let mut statements = vec![fresh(1), incref(1), wrap_source(), decref(1), read];
        statements.extend([rebuilt, decref(1), storage_dead(1)]);
        let body = elided(statements);
        assert_eq!(rc_ops_on_x(&body), 3, "{body}");
    }

    #[test]
    fn a_projected_source_keeps_the_pair() {
        let field = Place {
            local: Local(1),
            projection: vec![PlaceElem::Field(0)],
        };
        let statements = vec![
            fresh(1),
            stmt(StatementKind::IncRef(field.clone())),
            stmt(StatementKind::Assign(
                place(3),
                Rvalue::Use(Operand::Copy(field)),
            )),
            decref(1),
            storage_dead(1),
            decref(3),
            storage_dead(3),
        ];
        let body = elided(statements);
        let retains = body.basic_blocks[0]
            .statements
            .iter()
            .filter(|s| matches!(&s.kind, StatementKind::IncRef(_)))
            .count();
        assert_eq!(retains, 1, "{body}");
    }

    /// A release between the copy and the source's own release could be the
    /// last one once the pair is gone, moving where a drop hook runs.
    #[test]
    fn a_release_between_the_copy_and_the_source_release_keeps_the_pair() {
        let body = elided(dying(wrap_source(), vec![decref(2)], 3));
        assert_eq!(rc_ops_on_x(&body), 2, "{body}");
    }

    #[test]
    fn a_dying_local_with_a_drop_hook_hands_its_reference_over() {
        let resource = Type::new(TypeKind::Custom("Res".to_string(), None), span());
        let mut body = body_of(resource, dying(copy_source(false), Vec::new(), 3));
        body.local_decls[3] = body.local_decls[1].clone();
        body.has_drop_types.insert("Res".to_string());
        elide_rc(&mut body);
        assert_eq!(rc_ops_on_x(&body), 0, "{body}");
    }

    /// `_2 = Some(move _1)`.
    fn wrap_moved_source() -> Statement {
        stmt(StatementKind::Assign(
            place(2),
            Rvalue::Aggregate(
                AggregateKind::Option(optional_string_ty()),
                vec![Operand::Move(place(1))],
            ),
        ))
    }

    /// The verifier reads a moved operand as the aggregate taking the
    /// local's reference: a retain before it pays for the local, and with no
    /// retain the local's own release afterwards is one too many.
    #[test]
    fn the_verifier_reads_a_moved_aggregate_operand_as_taken_over() {
        let funded = vec![
            fresh(1),
            incref(1),
            wrap_moved_source(),
            decref(1),
            storage_dead(1),
            decref(2),
            storage_dead(2),
        ];
        let body = body_of(string_ty(), funded);
        assert!(verify_body(&body).is_empty(), "{:?}", verify_body(&body));

        let released_again = vec![
            fresh(1),
            wrap_moved_source(),
            decref(1),
            storage_dead(1),
            decref(2),
            storage_dead(2),
        ];
        let body = body_of(string_ty(), released_again);
        assert!(!verify_body(&body).is_empty(), "{body}");
    }

    #[test]
    fn a_borrowed_or_lending_source_keeps_the_pair() {
        for borrowed in [
            HashMap::from([(Local(1), vec![Local(3)])]),
            HashMap::from([(Local(3), vec![Local(1)])]),
        ] {
            let mut body = body_of(string_ty(), dying(wrap_source(), Vec::new(), 2));
            body.borrowed_locals = borrowed;
            elide_rc(&mut body);
            assert_eq!(rc_ops_on_x(&body), 2, "{body}");
        }
    }
}

mod constructed_trees {
    //! A node built from two subtrees: each subtree moves into the optional
    //! wrapping it, each optional moves into the constructor, and the
    //! constructor moves each into its field.

    use super::get_elided_bodies;
    use miri::mir::optimization::count_all_rc_ops;
    use miri::mir::symbol::Symbol;
    use miri::mir::{ArgMode, Body, StatementKind, TerminatorKind};
    use miri::type_checker::ModuleId;

    const TREES: &str = r#"
class Node
    var left Node?
    var right Node?

    fn init(left Node?, right Node?)
        self.left = left
        self.right = right

fn bottom_up(depth int) Node
    if depth == 0
        return Node(None, None)
    Node(bottom_up(depth - 1), bottom_up(depth - 1))

fn kept_leaf(leaf Node) Node
    let wrapped = Some(leaf)
    let parent = Node(wrapped, None)
    match wrapped
        Some(l): return l
        None: return parent
"#;

    fn body<'b>(bodies: &'b [(String, Body)], symbol: &Symbol) -> &'b Body {
        let name = symbol.link_name();
        &bodies
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .unwrap_or_else(|| panic!("{name} was not lowered"))
            .1
    }

    fn init_modes(body: &Body) -> Vec<Vec<ArgMode>> {
        body.basic_blocks
            .iter()
            .filter_map(|block| block.terminator.as_ref())
            .filter_map(|terminator| match &terminator.kind {
                TerminatorKind::Call { arg_modes, .. } if arg_modes.contains(&ArgMode::Owned) => {
                    Some(arg_modes.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn building_a_node_from_dying_subtrees_retains_and_releases_nothing() {
        let bodies = get_elided_bodies(TREES);
        let init = body(&bodies, &Symbol::method("Node", &[], "init", &[]));
        assert_eq!(init.owned_params, vec![false, true, true, false], "{init}");
        assert_eq!(count_all_rc_ops(init).0, 0, "{init}");

        let bottom_up = body(
            &bodies,
            &Symbol::function(&ModuleId::Program, "bottom_up", &[]),
        );
        assert_eq!(count_all_rc_ops(bottom_up), (0, 0), "{bottom_up}");
        assert!(
            init_modes(bottom_up)
                .iter()
                .all(|modes| modes[..3] == [ArgMode::Borrowed, ArgMode::Owned, ArgMode::Owned]),
            "{bottom_up}"
        );
    }

    /// The optional is read again after the node is built from it, so the
    /// call is funded by a retain of its own and the optional keeps its
    /// reference.
    #[test]
    fn a_subtree_read_after_the_node_is_built_keeps_its_retain() {
        let bodies = get_elided_bodies(TREES);
        let kept = body(
            &bodies,
            &Symbol::function(&ModuleId::Program, "kept_leaf", &[]),
        );
        let retains_before_a_call = kept.basic_blocks.iter().any(|block| {
            matches!(
                block.terminator.as_ref().map(|terminator| &terminator.kind),
                Some(TerminatorKind::Call { arg_modes, .. }) if arg_modes.contains(&ArgMode::Owned)
            ) && block
                .statements
                .iter()
                .any(|statement| matches!(statement.kind, StatementKind::IncRef(_)))
        });
        assert!(retains_before_a_call, "{kept}");
    }
}
