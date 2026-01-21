// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::mir::{BasicBlock, Body, Place, Rvalue, Statement, StatementKind, TerminatorKind};
use crate::mir::{BasicBlockData, Operand};
use std::collections::{HashMap, HashSet};

/// Transform the MIR body from SSA form back to standard form (remove Phis).
pub fn deconstruct_ssa(body: &mut Body) {
    let mut destructor = SSADestructor::new(body);
    destructor.run();
}

struct SSADestructor<'a> {
    body: &'a mut Body,
}

impl<'a> SSADestructor<'a> {
    fn new(body: &'a mut Body) -> Self {
        Self { body }
    }

    fn run(&mut self) {
        // 1. Collect all Phis and critical edges to split
        // We can't mutate body structure (split edges) whilst iterating it easily.
        // Also, splitting edges changes block indices?
        // No, we can append new blocks.
        // But adjusting terminators requires mutating blocks.

        // Approach:
        // Identify edges requiring split: (Pred, Succ).
        // Only split if Pred has > 1 successor AND Succ has Phis.
        // Actually, strictly: needed if we must insert code on the edge.
        // If Pred has 1 successor, we can insert in Pred.
        // If Pred has > 1 successor, we MUST split if we need to insert code for THIS edge.

        let edges_to_split = self.collect_edges_to_split();

        // 2. Split edges
        // We need to map old_edge -> new_intermediate_block because subsequent steps need to know where to insert.
        // Map: (Pred, Succ) -> InsertBlock
        let mut edge_map: HashMap<(BasicBlock, BasicBlock), BasicBlock> = HashMap::new();

        for (pred, succ) in edges_to_split {
            let new_bb = self.split_edge(pred, succ);
            edge_map.insert((pred, succ), new_bb);
        }

        // 3. Insert copies for Phis
        // We iterate all blocks, look for Phis.
        // For each phi argument (val, pred):
        //   Determine insertion block: edge_map.get(pred, current).unwrap_or(pred).
        //   Collect copies for that insertion block.

        let mut copies_per_block: HashMap<BasicBlock, Vec<(Place, Operand)>> = HashMap::new();

        for (bb_idx, block) in self.body.basic_blocks.iter().enumerate() {
            let bb = BasicBlock(bb_idx);
            for stmt in &block.statements {
                if let StatementKind::Assign(dest, Rvalue::Phi(args)) = &stmt.kind {
                    for (val, pred) in args {
                        let insert_block = edge_map.get(&(*pred, bb)).unwrap_or(pred);
                        copies_per_block
                            .entry(*insert_block)
                            .or_default()
                            .push((dest.clone(), val.clone()));
                    }
                }
            }
        }

        // 4. Sequentialize copies and insert statements
        for (block, copies) in copies_per_block {
            self.insert_copies(block, copies);
        }

        // 5. Remove Phis
        for block in &mut self.body.basic_blocks {
            block.statements.retain(|stmt| {
                if let StatementKind::Assign(_, Rvalue::Phi(_)) = &stmt.kind {
                    false
                } else {
                    true
                }
            });
        }
    }

    fn collect_edges_to_split(&self) -> Vec<(BasicBlock, BasicBlock)> {
        let mut edges = Vec::new();

        for (i, block) in self.body.basic_blocks.iter().enumerate() {
            let succ = BasicBlock(i);

            // Check if block has Phis
            let has_phi = block
                .statements
                .iter()
                .any(|s| matches!(s.kind, StatementKind::Assign(_, Rvalue::Phi(_))));

            if has_phi {
                // Check predecessors. We need to find predecessors.
                // But efficient way: iterate predecessors? We don't have predecessor map here (unless we build it).
                // Alternatively, iterate Phis arguments!
                // Phis list all predecessors.

                // Helper to get all predecessors from Phis
                // Assuming all Phis have same preds (properties of SSA).
                let mut preds = HashSet::new();
                for stmt in &block.statements {
                    if let StatementKind::Assign(_, Rvalue::Phi(args)) = &stmt.kind {
                        for (_, p) in args {
                            preds.insert(*p);
                        }
                    }
                }

                for pred in preds {
                    // Check if pred needs splitting.
                    // Pred needs splitting if it has > 1 successor OR if it terminates with Call (which forces split).
                    // Actually, if Pred has > 1 successor, we CANNOT insert on edge without split.

                    let pred_bb = &self.body.basic_blocks[pred.0];
                    if let Some(term) = &pred_bb.terminator {
                        let num_succs = term.successors().len();
                        // Also check for Call/GpuLaunch which require splitting even if 1 successor (because they write to destination, and we want copy to happen AFTER).
                        // Wait, Call writes to destination. Jump happens after.
                        // Can we insert `x = ...` after Call but before Jump?
                        // Terminator IS the Call.
                        // So we cannot insert AFTER terminator.
                        // So Call terminators MUST split edge to insert code.

                        let needs_split = num_succs > 1
                            || matches!(
                                term.kind,
                                TerminatorKind::Call { .. } | TerminatorKind::GpuLaunch { .. }
                            );

                        if needs_split {
                            edges.push((pred, succ));
                        }
                    }
                }
            }
        }
        edges
    }

    fn split_edge(&mut self, pred: BasicBlock, succ: BasicBlock) -> BasicBlock {
        // Create new block
        let new_bb = BasicBlock(self.body.basic_blocks.len());
        let new_block_data = BasicBlockData {
            statements: Vec::new(),
            terminator: Some(crate::mir::Terminator {
                kind: TerminatorKind::Goto { target: succ },
                span: Span::default(),
            }),
            is_cleanup: false,
        };
        self.body.basic_blocks.push(new_block_data);

        // Redirect pred -> succ to pred -> new_bb
        let pred_block = &mut self.body.basic_blocks[pred.0];
        if let Some(term) = &mut pred_block.terminator {
            term.replace_successor(succ, new_bb);
        }

        new_bb
    }

    fn insert_copies(&mut self, block: BasicBlock, copies: Vec<(Place, Operand)>) {
        // Simple sequentialization: verify if any Dest is used in any Source.
        // If so, use temp.
        // Or simpler: always use temp for one side if needed?
        // Let's implement robust "read to temps, write from temps" if simplified.
        // But pure "read to temps" doubles variables for no reason.

        // Dependency Graph approach?
        // Or simple:
        // `pending_assignments`: `dest = src`.
        // To avoid cycles `a=b, b=a`:
        // Break cycle with temp.

        // Algorithm:
        // While `copies` not empty:
        //   Find a copy `(d, s)` where `d` is NOT used in any other copy's `s`.
        //   If found: emit `d = s`, remove from set.
        //   If not found (cycle): Pick any `(d, s)`, emit `temp = s`, `d = temp`.
        //     Wait, breaking cycle `a=b, b=a`.
        //     `t = b`. `a = t`? No.
        //     We want result: `a_new = b_old`, `b_new = a_old`.
        //     If we do `a = b` (a gets b), then `b = a` (b gets a which is b). Wrong.
        //     We need to save `a` if it's overwritten.

        // Let's assume copies are `(dest, src)`.
        // `src` are values *before* this block?
        // No, SSA values are unique.
        // Wait! In SSA, `v1` and `v2` are distinct versions.
        // `x0 = 1, x1 = 2`.
        // `x2 = phi(x0, x1)`.
        // `pred0`: `x2 = x0`.
        // `x2` is fresh. `x0` is old. They don't overlap!
        // IN SSA DESTRUCTION, if we map back to *same* physical registers/locals, we have overlap.
        // But here we are just outputting standard MIR with *still the same SSA locals*.
        // We are NOT doing register allocation or coalescing yet.
        // So `dest` (PHI LHS) is ALWAYS a fresh variable defined at PHI.
        // `src` (PHI RHS) is defined in predecessor.
        // THEY ARE DISTINCT NAMES.
        // So there are NO cycles possible in terms of SSA names!
        // We can just emit `dest = src` in any order.

        // EXCEPTION: If we had "Lost Copy" due to critical edge handling optimization?
        // But we handle critical edges.

        // So simple emission is safe!

        let bb = &mut self.body.basic_blocks[block.0];

        // Insert at end of statements (before terminator)
        // Or if block was just created for split, it's empty.
        // If block is Pred, we append.

        for (dest, src) in copies {
            let stmt = Statement {
                kind: StatementKind::Assign(dest, Rvalue::Use(src)),
                span: Default::default(),
            };

            // Should insert before terminator.
            // But statements vector assumes execution before terminator.
            bb.statements.push(stmt);
        }
    }
}
