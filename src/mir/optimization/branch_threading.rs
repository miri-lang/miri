// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lets a block branch on the condition it computed, rather than jump to a
//! join block that branches on a copy of it.
//!
//! `a and b`, `a or b` and every condition built from them lower to blocks
//! that each assign the condition to one local — the comparison on one path,
//! a constant on the other — and jump to a join block holding only copies of
//! that local and a `switchInt` on the copy. Code generation then merges the
//! condition into a value at the join and compares that value to branch, so a
//! loop guard `i < n and j < m` costs a materialised boolean per test.
//!
//! The join block is copied into each block that jumps to it: its copies run
//! there, so every later read of the locals sees the same values, and the
//! block ends with the join's `switchInt` itself. Where the switched value is
//! a constant known in that block, the branch becomes a jump to the target it
//! would take. A join block qualifies only when every statement copies a
//! boolean local or constant into a boolean local, so copying it moves no
//! reference count, no storage marker and no effect; a block that jumps to
//! itself is left alone.

use std::collections::HashMap;

use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::mir::{
    BasicBlock, Body, Discriminant, Local, Operand, Rvalue, Statement, StatementKind,
    TerminatorKind,
};

/// Copies every qualifying join block into the blocks that jump to it.
/// Returns whether any block changed.
pub fn thread_branch_conditions(body: &mut Body) -> bool {
    let mut changed = false;
    for index in 0..body.basic_blocks.len() {
        let Some(target) = goto_target(body, index) else {
            continue;
        };
        if target != index && is_branch_on_copies(body, target) {
            inline_join(body, index, target);
            changed = true;
        }
    }
    changed
}

/// The block `index` jumps to unconditionally, if it ends in a `Goto`.
fn goto_target(body: &Body, index: usize) -> Option<usize> {
    match &body.basic_blocks[index].terminator.as_ref()?.kind {
        TerminatorKind::Goto { target } => Some(target.0),
        _ => None,
    }
}

/// Whether block `index` holds only boolean copies and ends in a
/// `switchInt` on a boolean local.
fn is_branch_on_copies(body: &Body, index: usize) -> bool {
    let block = &body.basic_blocks[index];
    let Some(terminator) = &block.terminator else {
        return false;
    };
    let switches_on_local = matches!(
        &terminator.kind,
        TerminatorKind::SwitchInt { discr: Operand::Copy(place) | Operand::Move(place), .. }
            if place.projection.is_empty() && is_boolean_local(body, place.local)
    );
    switches_on_local && block.statements.iter().all(|s| is_boolean_copy(body, s))
}

/// Whether `statement` assigns a boolean local or constant to a whole
/// boolean local.
fn is_boolean_copy(body: &Body, statement: &Statement) -> bool {
    let StatementKind::Assign(place, Rvalue::Use(operand)) = &statement.kind else {
        return false;
    };
    let source_is_boolean = match operand {
        Operand::Copy(source) | Operand::Move(source) => {
            source.projection.is_empty() && is_boolean_local(body, source.local)
        }
        Operand::Constant(constant) => matches!(constant.literal, Literal::Boolean(_)),
        Operand::Function(_) => false,
    };
    place.projection.is_empty() && is_boolean_local(body, place.local) && source_is_boolean
}

fn is_boolean_local(body: &Body, local: Local) -> bool {
    body.local_decls
        .get(local.0)
        .is_some_and(|decl| decl.ty.kind == TypeKind::Boolean)
}

/// Appends the join block's copies to block `index` and gives it the join's
/// branch, folded to a jump when the switched local holds a known constant.
fn inline_join(body: &mut Body, index: usize, join: usize) {
    let copies = body.basic_blocks[join].statements.clone();
    let branch = body.basic_blocks[join].terminator.clone();
    let block = &mut body.basic_blocks[index];
    block.statements.extend(copies);
    block.terminator = branch;
    let known = known_booleans(&block.statements);
    if let Some(terminator) = block.terminator.as_mut() {
        if let Some(target) = constant_branch_target(&terminator.kind, &known) {
            terminator.kind = TerminatorKind::Goto { target };
        }
    }
}

/// The boolean value each local holds at the end of `statements`, where it is
/// known from a constant assigned in them, directly or through copies.
fn known_booleans(statements: &[Statement]) -> HashMap<Local, bool> {
    let mut known = HashMap::new();
    for statement in statements {
        let StatementKind::Assign(place, rvalue) = &statement.kind else {
            continue;
        };
        if !place.projection.is_empty() {
            continue;
        }
        let value = match rvalue {
            Rvalue::Use(Operand::Constant(constant)) => match constant.literal {
                Literal::Boolean(value) => Some(value),
                _ => None,
            },
            Rvalue::Use(Operand::Copy(source) | Operand::Move(source))
                if source.projection.is_empty() =>
            {
                known.get(&source.local).copied()
            }
            _ => None,
        };
        match value {
            Some(value) => known.insert(place.local, value),
            None => known.remove(&place.local),
        };
    }
    known
}

/// The block a `switchInt` on a local of known value goes to.
fn constant_branch_target(
    kind: &TerminatorKind,
    known: &HashMap<Local, bool>,
) -> Option<BasicBlock> {
    let TerminatorKind::SwitchInt {
        discr: Operand::Copy(place) | Operand::Move(place),
        targets,
        otherwise,
    } = kind
    else {
        return None;
    };
    let value = Discriminant::new(u128::from(*known.get(&place.local)?));
    let hit = targets.iter().find(|(case, _)| *case == value);
    Some(hit.map_or(*otherwise, |(_, target)| *target))
}
