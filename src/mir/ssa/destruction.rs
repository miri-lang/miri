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
        // Split critical edges (pred has >1 successor) where PHI copies are needed,
        // then replace PHI nodes with copy assignments in predecessor blocks.
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
            block
                .statements
                .retain(|stmt| !matches!(stmt.kind, StatementKind::Assign(_, Rvalue::Phi(_))));
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
                        // Call/GpuLaunch terminators also need splitting: we cannot insert
                        // copies after a terminator, so a new block is required.

                        let needs_split = num_succs > 1
                            || matches!(
                                term.kind,
                                TerminatorKind::Call { .. }
                                    | TerminatorKind::GpuLaunch { .. }
                                    | TerminatorKind::VirtualCall { .. }
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
        // Simple sequential emission is safe: in SSA, PHI destinations are always
        // fresh locals distinct from sources, so no ordering cycles are possible.
        // Critical edges are already split, preventing lost-copy issues.
        let bb = &mut self.body.basic_blocks[block.0];

        for (dest, src) in copies {
            bb.statements.push(Statement {
                kind: StatementKind::Assign(dest, Rvalue::Use(src)),
                span: Default::default(),
            });
        }
    }
}
