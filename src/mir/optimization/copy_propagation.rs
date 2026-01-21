// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::OptimizationPass;
use crate::mir::block::BasicBlock;
use crate::mir::operand::Operand;
use crate::mir::place::Local;
use crate::mir::rvalue::Rvalue;
use crate::mir::statement::StatementKind;
use crate::mir::visitor::MutVisitor;
use crate::mir::Body;
use std::collections::HashMap;

pub struct CopyPropagation;

struct Replacer<'a> {
    replacements: &'a HashMap<Local, Operand>,
    changed: bool,
}

impl<'a> MutVisitor for Replacer<'a> {
    fn visit_operand(&mut self, operand: &mut Operand, _loc: BasicBlock) {
        if let Operand::Copy(place) | Operand::Move(place) = operand {
            if place.projection.is_empty() {
                if let Some(replacement) = self.replacements.get(&place.local) {
                    *operand = replacement.clone();
                    self.changed = true;
                }
            }
        }
    }
}

impl OptimizationPass for CopyPropagation {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;

        for (i, block) in body.basic_blocks.iter_mut().enumerate() {
            let block_id = BasicBlock(i);
            let mut replacements: HashMap<Local, Operand> = HashMap::new();

            for stmt in &mut block.statements {
                // 1. Apply replacements
                let mut replacer = Replacer {
                    replacements: &replacements,
                    changed: false,
                };
                replacer.visit_statement(block_id, stmt);
                if replacer.changed {
                    changed = true;
                }

                // 2. Update replacements
                if let StatementKind::Assign(place, rvalue) = &stmt.kind {
                    if place.projection.is_empty() {
                        let dest = place.local;

                        // Invalidate any replacement that depends on dest
                        replacements.retain(|_, op| !uses_local(op, dest));

                        if let Rvalue::Use(op) = rvalue {
                            if is_simple_operand(op) {
                                replacements.insert(dest, op.clone());
                            } else {
                                replacements.remove(&dest);
                            }
                        } else {
                            replacements.remove(&dest);
                        }
                    } else {
                        // Assignment to projection. Might invalidate things?
                        let dest = place.local;
                        if replacements.contains_key(&dest) {
                            replacements.remove(&dest);
                        }
                        // Also invalidate any dependencies on 'dest'
                        replacements.retain(|_, op| !uses_local(op, dest));
                    }
                }
            }

            if let Some(terminator) = &mut block.terminator {
                let mut replacer = Replacer {
                    replacements: &replacements,
                    changed: false,
                };
                replacer.visit_terminator(block_id, terminator);
                if replacer.changed {
                    changed = true;
                }
            }
        }

        changed
    }

    fn name(&self) -> &'static str {
        "Copy Propagation"
    }
}

fn uses_local(op: &Operand, target: Local) -> bool {
    match op {
        Operand::Copy(p) | Operand::Move(p) => p.local == target,
        Operand::Constant(_) => false,
    }
}

fn is_simple_operand(_op: &Operand) -> bool {
    // We propagate any operand that fits in Operand enum
    true
}
