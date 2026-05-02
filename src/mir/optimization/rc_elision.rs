// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! RC elision pass: removes redundant IncRef/DecRef pairs.
//!
//! After the Perceus pass inserts RC operations, this pass removes pairs of
//! `IncRef(src)` + `DecRef(dest)` that are provably redundant. A pair is
//! redundant when `dest = Copy(src)` and neither the IncRef nor the DecRef
//! has any net effect on the program's memory safety:
//!
//! - The IncRef increments the RC to account for the copy alias.
//! - The DecRef decrements it back when the alias dies.
//! - Net effect: zero.
//!
//! # Safety conditions
//!
//! Elision of the pair `(IncRef(src), DecRef(dest))` is safe when:
//!
//! 1. Both operations are in the **same basic block** (scope-local).
//! 2. `src` does not have a user-defined destructor (`has_drop` types are skipped).
//!    Elision is safe only for types where RC reaching 0 has no side effects
//!    beyond freeing memory.
//! 3. `DecRef(src)` does not appear between the copy assignment and `DecRef(dest)`.
//!    If src's reference count reaches zero before dest's alias is cleaned up,
//!    dest would hold a dangling pointer after elision.
//!
//! # Example
//!
//! Before elision (after Perceus):
//! ```text
//! IncRef(_1)           // _1 is a List param
//! _3 = Copy(_1)
//! _4 = Copy(_3[0])     // index read — int, not managed
//! DecRef(_3)
//! StorageDead(_3)
//! ```
//!
//! After elision:
//! ```text
//! _3 = Copy(_1)
//! _4 = Copy(_3[0])
//! StorageDead(_3)      // no DecRef — _3 held no owned reference
//! ```
//!
//! The caller's RC for `_1` is unchanged; the object is freed by the caller.

use crate::mir::optimization::OptimizationPass;
use crate::mir::place::Local;
use crate::mir::statement::StatementKind;
use crate::mir::types::MirType;
use crate::mir::{Body, Operand, Place};
use std::collections::HashSet;

pub struct RcElision;

impl OptimizationPass for RcElision {
    fn run(&mut self, body: &mut Body) -> bool {
        let has_drop_types = &body.has_drop_types.clone();
        let local_decls = &body.local_decls.clone();

        let mut any_changed = false;
        for block in &mut body.basic_blocks {
            if elide_block(block, has_drop_types, local_decls) {
                any_changed = true;
            }
        }
        any_changed
    }

    fn name(&self) -> &'static str {
        "RC Elision"
    }
}

/// Process one basic block, removing provably redundant (IncRef, DecRef) pairs.
///
/// Returns true if any statements were removed.
fn elide_block(
    block: &mut crate::mir::block::BasicBlockData,
    has_drop_types: &HashSet<String>,
    local_decls: &[crate::mir::LocalDecl],
) -> bool {
    let stmts = &block.statements;
    let n = stmts.len();

    // Positions to remove (IncRef and DecRef positions that form a redundant pair).
    let mut to_remove: HashSet<usize> = HashSet::new();

    // Scan for pattern:
    //   pos i:   IncRef(src)
    //   pos i+1: Assign(dest, Use(Copy(src)))  (no projections on either side)
    //   pos j:   DecRef(dest)                  (j > i+1, same block)
    // with safety conditions:
    //   - src's type has no destructor
    //   - no DecRef(src) in statements [i+2, j)
    for i in 0..n.saturating_sub(1) {
        // Already marked for removal — skip.
        if to_remove.contains(&i) {
            continue;
        }

        let src_local = match &stmts[i].kind {
            StatementKind::IncRef(p) if p.projection.is_empty() => p.local,
            _ => continue,
        };

        // Next statement must be Assign(dest, Use(Copy(src))) — no projections.
        let dest_local = match &stmts[i + 1].kind {
            StatementKind::Assign(dest, rvalue) if dest.projection.is_empty() => {
                if let Some(src_place) = copy_source(rvalue) {
                    if src_place.local == src_local && src_place.projection.is_empty() {
                        dest.local
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        // src's type must not carry a user-defined destructor.
        if type_has_destructor(src_local, local_decls, has_drop_types) {
            continue;
        }

        // Find DecRef(dest_local) later in the block (first occurrence after i+1).
        let j = match find_decref(stmts, dest_local, i + 2) {
            Some(pos) => pos,
            None => continue,
        };

        // Safety check: no DecRef(src) in statements [i+2, j).
        if decref_in_range(stmts, src_local, i + 2, j) {
            continue;
        }

        to_remove.insert(i);
        to_remove.insert(j);
    }

    if to_remove.is_empty() {
        return false;
    }

    // Rebuild the statement list without the marked positions.
    let old_stmts = std::mem::take(&mut block.statements);
    block.statements = old_stmts
        .into_iter()
        .enumerate()
        .filter(|(pos, _)| !to_remove.contains(pos))
        .map(|(_, s)| s)
        .collect();

    true
}

/// Extract the source place if the rvalue is `Use(Copy(place))`.
fn copy_source(rvalue: &crate::mir::Rvalue) -> Option<&Place> {
    match rvalue {
        crate::mir::Rvalue::Use(Operand::Copy(p)) => Some(p),
        _ => None,
    }
}

/// Returns true if `local`'s MIR type is a Custom type with a user destructor.
fn type_has_destructor(
    local: Local,
    local_decls: &[crate::mir::LocalDecl],
    has_drop_types: &HashSet<String>,
) -> bool {
    match &local_decls[local.0].mir_ty {
        MirType::Custom(name) => has_drop_types.contains(name.as_str()),
        _ => false,
    }
}

/// Find the first `DecRef(local)` at or after `start_pos`, with no projection.
fn find_decref(stmts: &[crate::mir::Statement], local: Local, start_pos: usize) -> Option<usize> {
    for (i, stmt) in stmts.iter().enumerate().skip(start_pos) {
        if let StatementKind::DecRef(p) = &stmt.kind {
            if p.local == local && p.projection.is_empty() {
                return Some(i);
            }
        }
    }
    None
}

/// Returns true if `DecRef(local)` appears in statements `[start, end)`.
fn decref_in_range(
    stmts: &[crate::mir::Statement],
    local: Local,
    start: usize,
    end: usize,
) -> bool {
    stmts[start..end.min(stmts.len())]
        .iter()
        .any(|stmt| matches!(&stmt.kind, StatementKind::DecRef(p) if p.local == local && p.projection.is_empty()))
}

// ─── RC op counters (used by tests) ─────────────────────────────────────────

/// Count IncRef and DecRef operations for a specific local across all blocks.
///
/// Used by tests to assert that RC operations were eliminated.
pub fn count_rc_ops(body: &Body, local: Local) -> (usize, usize) {
    let mut incref = 0usize;
    let mut decref = 0usize;
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::IncRef(p) if p.local == local => incref += 1,
                StatementKind::DecRef(p) if p.local == local => decref += 1,
                _ => {}
            }
        }
    }
    (incref, decref)
}

/// Count ALL IncRef and DecRef operations across all blocks in the body.
///
/// Used by tests to verify that a linear program has zero RC operations.
pub fn count_all_rc_ops(body: &Body) -> (usize, usize) {
    let mut incref = 0usize;
    let mut decref = 0usize;
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::IncRef(_) => incref += 1,
                StatementKind::DecRef(_) => decref += 1,
                _ => {}
            }
        }
    }
    (incref, decref)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::syntax::Span;
    use crate::mir::statement::StatementKind;
    use crate::mir::terminator::TerminatorKind;
    use crate::mir::{Place, Statement};

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
                crate::mir::Rvalue::Use(Operand::Copy(Place {
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

    fn make_local_decls_string(count: usize) -> Vec<crate::mir::LocalDecl> {
        use crate::ast::types::{Type, TypeKind};
        use crate::mir::LocalDecl;
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

    fn make_block(stmts: Vec<Statement>) -> crate::mir::block::BasicBlockData {
        crate::mir::block::BasicBlockData {
            statements: stmts,
            terminator: Some(crate::mir::Terminator {
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
}
