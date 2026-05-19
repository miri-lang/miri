// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::OptimizationPass;
use crate::mir::block::BasicBlock;
use crate::mir::terminator::TerminatorKind;
use crate::mir::Body;
use std::collections::{HashMap, HashSet};

/// Simplifies the control flow graph by threading empty jump-only blocks
/// and removing unreachable blocks.
///
/// Pass 1 (thread jumps): If block B is empty and terminates with `Goto(C)`,
/// all jumps to B are redirected to C. Chains are resolved iteratively.
///
/// Pass 2 (remove unreachable): Blocks not reachable from the entry block
/// are removed and all block indices are remapped.
pub struct SimplifyCfg;

impl OptimizationPass for SimplifyCfg {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;

        // 1. Thread jumps (A -> B -> C => A -> C) where B is empty
        if thread_jumps(body) {
            changed = true;
        }

        // 2. Remove unreachable blocks
        if remove_unreachable_blocks(body) {
            changed = true;
        }

        changed
    }

    fn name(&self) -> &'static str {
        "Simplify CFG"
    }
}

fn thread_jumps(body: &mut Body) -> bool {
    let mut replacements = collect_goto_only_blocks(body);
    if replacements.is_empty() {
        return false;
    }
    resolve_replacement_chains(&mut replacements);
    apply_block_replacements(body, &replacements)
}

/// Identify blocks that are empty and end with `Goto(target)` — they can be
/// short-circuited away. Self-loops are skipped to avoid infinite threading.
fn collect_goto_only_blocks(body: &Body) -> HashMap<BasicBlock, BasicBlock> {
    let mut replacements = HashMap::new();
    for (i, block) in body.basic_blocks.iter().enumerate() {
        if !block.statements.is_empty() {
            continue;
        }
        let Some(term) = &block.terminator else {
            continue;
        };
        if let TerminatorKind::Goto { target } = term.kind {
            if target != BasicBlock(i) {
                replacements.insert(BasicBlock(i), target);
            }
        }
    }
    replacements
}

/// Iteratively resolve `A -> B, B -> C` chains into `A -> C`.
fn resolve_replacement_chains(replacements: &mut HashMap<BasicBlock, BasicBlock>) {
    loop {
        let mut progress = false;
        for key in replacements.keys().copied().collect::<Vec<_>>() {
            let target = replacements[&key];
            if let Some(&next) = replacements.get(&target) {
                if next != target {
                    replacements.insert(key, next);
                    progress = true;
                }
            }
        }
        if !progress {
            break;
        }
    }
}

fn apply_block_replacements(
    body: &mut Body,
    replacements: &HashMap<BasicBlock, BasicBlock>,
) -> bool {
    let mut changed = false;
    for block in &mut body.basic_blocks {
        let Some(terminator) = &mut block.terminator else {
            continue;
        };
        match &mut terminator.kind {
            TerminatorKind::Goto { target } => {
                if let Some(new_target) = replacements.get(target) {
                    *target = *new_target;
                    changed = true;
                }
            }
            TerminatorKind::SwitchInt {
                targets, otherwise, ..
            } => {
                for (_, target) in targets {
                    if let Some(new_target) = replacements.get(target) {
                        *target = *new_target;
                        changed = true;
                    }
                }
                if let Some(new_target) = replacements.get(otherwise) {
                    *otherwise = *new_target;
                    changed = true;
                }
            }
            TerminatorKind::Call { target, .. }
            | TerminatorKind::GpuLaunch { target, .. }
            | TerminatorKind::VirtualCall { target, .. } => {
                if let Some(t) = target {
                    if let Some(new_target) = replacements.get(t) {
                        *t = *new_target;
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

fn remove_unreachable_blocks(body: &mut Body) -> bool {
    let mut reachable = HashSet::new();
    let mut worklist = vec![BasicBlock(0)];
    reachable.insert(BasicBlock(0));

    while let Some(bb) = worklist.pop() {
        if bb.0 >= body.basic_blocks.len() {
            continue; // Safety check
        }
        if let Some(term) = &body.basic_blocks[bb.0].terminator {
            let successors = term.successors();
            for succ in successors {
                if reachable.insert(succ) {
                    worklist.push(succ);
                }
            }
        }
    }

    if reachable.len() == body.basic_blocks.len() {
        return false;
    }

    // Map old index -> new index
    let mut map: HashMap<BasicBlock, BasicBlock> = HashMap::new();
    let mut new_blocks = Vec::new();
    let mut new_index = 0;

    for (i, block) in body.basic_blocks.drain(..).enumerate() {
        if reachable.contains(&BasicBlock(i)) {
            map.insert(BasicBlock(i), BasicBlock(new_index));
            new_blocks.push(block);
            new_index += 1;
        }
    }

    body.basic_blocks = new_blocks;

    // Remap terminators in new blocks
    for block in &mut body.basic_blocks {
        if let Some(terminator) = &mut block.terminator {
            match &mut terminator.kind {
                TerminatorKind::Goto { target } => {
                    *target = map[target];
                }
                TerminatorKind::SwitchInt {
                    targets, otherwise, ..
                } => {
                    for (_, target) in targets {
                        *target = map[target];
                    }
                    *otherwise = map[otherwise];
                }
                TerminatorKind::Call { target, .. }
                | TerminatorKind::GpuLaunch { target, .. }
                | TerminatorKind::VirtualCall { target, .. } => {
                    if let Some(t) = target {
                        *t = map[t];
                    }
                }
                _ => {}
            }
        }
    }

    true
}
