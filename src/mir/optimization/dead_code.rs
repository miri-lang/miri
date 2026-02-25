// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::OptimizationPass;
use crate::mir::block::BasicBlock;
use crate::mir::place::{Local, PlaceContext};
use crate::mir::rvalue::{GpuIntrinsic, Rvalue};
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::visitor::Visitor;
use crate::mir::Body;
use std::collections::HashSet;

/// Removes assignments to locals that are never read.
///
/// This pass iterates to a fixpoint: after removing dead assignments, previously
/// "used" locals may become dead. Only side-effect-free rvalues are removed.
/// The return place `_0` is always considered live.
pub struct DeadCodeElimination;

struct UsedLocalsCollector {
    used: HashSet<Local>,
}

impl UsedLocalsCollector {
    fn new() -> Self {
        Self {
            used: HashSet::new(),
        }
    }
}

impl Visitor for UsedLocalsCollector {
    fn visit_local(&mut self, local: Local, context: PlaceContext, _loc: BasicBlock) {
        // Collect locals that are used in a way that implies they are needed.
        // NonMutatingUse is a read.
        if context == PlaceContext::NonMutatingUse {
            self.used.insert(local);
        }
    }
}

impl OptimizationPass for DeadCodeElimination {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;
        loop {
            let mut collector = UsedLocalsCollector::new();
            collector.visit_body(body);
            // _0 is return value substitute, always used (implicitly)
            collector.used.insert(Local(0));

            let mut iteration_changed = false;

            for block_data in &mut body.basic_blocks {
                for stmt in &mut block_data.statements {
                    if let StatementKind::Assign(place, rvalue) = &stmt.kind {
                        // Check if we can remove this assignment.
                        // We can remove if:
                        // 1. place is a Local (no projection)
                        // 2. place.local is NOT in collector.used
                        // 3. rvalue is side-effect free.
                        if place.projection.is_empty()
                            && !collector.used.contains(&place.local)
                            && is_side_effect_free(rvalue)
                        {
                            *stmt = Statement {
                                kind: StatementKind::Nop,
                                span: stmt.span,
                            };
                            iteration_changed = true;
                        }
                    }
                }
            }

            if iteration_changed {
                changed = true;
            } else {
                break;
            }
        }
        changed
    }

    fn name(&self) -> &'static str {
        "Dead Code Elimination"
    }
}

fn is_side_effect_free(rvalue: &Rvalue) -> bool {
    match rvalue {
        Rvalue::Use(_)
        | Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::Aggregate(..)
        | Rvalue::Phi(_) => true,
        Rvalue::GpuIntrinsic(intrinsic) => !matches!(intrinsic, GpuIntrinsic::SyncThreads),
        Rvalue::Allocate(..) => false,
    }
}
