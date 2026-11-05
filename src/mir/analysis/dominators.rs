// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::analysis::traversal::reverse_postorder;
use crate::mir::{BasicBlock, Body, TerminatorKind};
use std::collections::{HashMap, HashSet};

/// Represents the Dominator Tree of a function body.
pub struct DominatorTree {
    /// Map from a block to its immediate dominator.
    /// The entry block has no immediate dominator.
    pub immediate_dominators: HashMap<BasicBlock, BasicBlock>,
    /// Map from a block to the set of blocks in its dominance frontier.
    pub dominance_frontiers: HashMap<BasicBlock, HashSet<BasicBlock>>,
    /// Dominator tree children (inverse of immediate_dominators) for traversal.
    pub children: HashMap<BasicBlock, Vec<BasicBlock>>,
}

impl DominatorTree {
    /// Compute the dominator tree for the given body.
    pub fn compute(body: &Body) -> Self {
        let predecessors = compute_predecessors(body);
        let immediate_dominators = compute_immediate_dominators(body, &predecessors);
        let dominance_frontiers =
            compute_dominance_frontiers(body, &predecessors, &immediate_dominators);

        // Compute children map
        let mut children: HashMap<BasicBlock, Vec<BasicBlock>> = HashMap::new();
        for (&node, &dom) in &immediate_dominators {
            if node != dom {
                // Avoid root self-loop if any, typically idom[root] is not in map or is root
                children.entry(dom).or_default().push(node);
            }
        }
        // Ensure consistent order for deterministic compilation
        for kids in children.values_mut() {
            kids.sort_by_key(|bb| bb.0);
        }

        Self {
            immediate_dominators,
            dominance_frontiers,
            children,
        }
    }

    /// Check if block `a` dominates block `b`.
    /// `a` dominates `b` if every path from the entry to `b` goes through `a`.
    /// By definition, a block dominates itself.
    pub fn dominates(&self, a: BasicBlock, mut b: BasicBlock) -> bool {
        if a == b {
            return true;
        }

        // Walk up the dominator tree from b
        while let Some(&idom) = self.immediate_dominators.get(&b) {
            if idom == a {
                return true;
            }
            if idom == b {
                // Should not happen in a tree, but prevents infinite loop if cycles exist
                break;
            }
            b = idom;
        }

        false
    }
}

/// Compute predecessors for each block in the CFG.
fn compute_predecessors(body: &Body) -> HashMap<BasicBlock, Vec<BasicBlock>> {
    let mut predecessors: HashMap<BasicBlock, Vec<BasicBlock>> = HashMap::new();

    // Initialize empty lists for all blocks
    for (i, _) in body.basic_blocks.iter().enumerate() {
        predecessors.insert(BasicBlock(i), Vec::new());
    }

    for (i, block) in body.basic_blocks.iter().enumerate() {
        let source_bb = BasicBlock(i);
        if let Some(terminator) = &block.terminator {
            let targets = match &terminator.kind {
                TerminatorKind::Goto { target } => vec![*target],
                TerminatorKind::SwitchInt {
                    targets, otherwise, ..
                } => {
                    let mut succs = Vec::new();
                    for (_, target) in targets {
                        succs.push(*target);
                    }
                    succs.push(*otherwise);
                    succs
                }
                TerminatorKind::Return | TerminatorKind::Unreachable => vec![],
                TerminatorKind::Call { target, .. }
                | TerminatorKind::GpuLaunch { target, .. }
                | TerminatorKind::VirtualCall { target, .. } => {
                    if let Some(t) = target {
                        vec![*t]
                    } else {
                        vec![]
                    }
                }
            };

            for target in targets {
                if let Some(preds) = predecessors.get_mut(&target) {
                    preds.push(source_bb);
                }
            }
        }
    }

    predecessors
}

/// Compute immediate dominators with the Cooper-Harvey-Kennedy iterative
/// algorithm.
///
/// Blocks are visited, and compared, by their reverse-postorder number, not
/// by their index. A pass may append a block (a loop preheader) after blocks
/// it dominates, so an index says nothing about where a block sits in the
/// graph, and intersecting on indices would report less dominance than holds.
/// A block the entry cannot reach has no number, gets no dominator, and is
/// never counted as a predecessor.
fn compute_immediate_dominators(
    body: &Body,
    predecessors: &HashMap<BasicBlock, Vec<BasicBlock>>,
) -> HashMap<BasicBlock, BasicBlock> {
    let entry_node = BasicBlock(0);
    let order = reverse_postorder(body);
    let mut rpo_number = vec![None; body.basic_blocks.len()];
    for (number, &block) in order.iter().enumerate() {
        rpo_number[block] = Some(number);
    }

    let mut idoms: HashMap<BasicBlock, BasicBlock> = HashMap::new();
    let mut changed = true;
    while changed {
        changed = false;
        for &block in order.iter().skip(1) {
            let node = BasicBlock(block);
            let mut processed = predecessors[&node].iter().copied().filter(|&pred| {
                rpo_number[pred.0].is_some() && (pred == entry_node || idoms.contains_key(&pred))
            });
            let Some(first) = processed.next() else {
                continue;
            };
            let candidate = processed.fold(first, |candidate, pred| {
                intersect(&idoms, &rpo_number, candidate, pred)
            });
            if idoms.insert(node, candidate) != Some(candidate) {
                changed = true;
            }
        }
    }

    idoms
}

/// The nearest common dominator of two blocks in the tree built so far: the
/// Cooper-Harvey-Kennedy finger walk, each finger climbing while it sits
/// later in reverse postorder than the other.
fn intersect(
    idoms: &HashMap<BasicBlock, BasicBlock>,
    rpo_number: &[Option<usize>],
    b1: BasicBlock,
    b2: BasicBlock,
) -> BasicBlock {
    let number = |block: BasicBlock| rpo_number[block.0].unwrap_or(0);
    let mut finger1 = b1;
    let mut finger2 = b2;
    while finger1 != finger2 {
        while number(finger1) > number(finger2) {
            match idoms.get(&finger1) {
                Some(&parent) => finger1 = parent,
                None => return BasicBlock(0),
            }
        }
        while number(finger2) > number(finger1) {
            match idoms.get(&finger2) {
                Some(&parent) => finger2 = parent,
                None => return BasicBlock(0),
            }
        }
    }
    finger1
}

/// Compute dominance frontiers.
/// DF(n) = { m | n dominates a pred of m, but n does not strictly dominate m }
fn compute_dominance_frontiers(
    body: &Body,
    predecessors: &HashMap<BasicBlock, Vec<BasicBlock>>,
    idoms: &HashMap<BasicBlock, BasicBlock>,
) -> HashMap<BasicBlock, HashSet<BasicBlock>> {
    let mut frontiers: HashMap<BasicBlock, HashSet<BasicBlock>> = HashMap::new();

    for (i, _) in body.basic_blocks.iter().enumerate() {
        frontiers.insert(BasicBlock(i), HashSet::new());
    }

    for (i, _) in body.basic_blocks.iter().enumerate() {
        let node = BasicBlock(i);
        let preds = &predecessors[&node];

        if preds.len() >= 2 {
            for &p in preds {
                let mut runner = p;
                while runner != *idoms.get(&node).unwrap_or(&BasicBlock(0)) {
                    // Add node to runner's frontier
                    if let Some(frontier) = frontiers.get_mut(&runner) {
                        frontier.insert(node);
                    }

                    if let Some(&parent) = idoms.get(&runner) {
                        runner = parent;
                    } else {
                        // Reached root
                        if runner.0 != 0 {
                            // Should be unreachable code handling
                            break;
                        }
                        break;
                    }
                }
            }
        }
    }

    frontiers
}
