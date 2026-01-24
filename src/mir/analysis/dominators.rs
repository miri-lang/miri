// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
                TerminatorKind::Call { target, .. } | TerminatorKind::GpuLaunch { target, .. } => {
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

/// Compute immediate dominators using the iterative algorithm.
/// This is a simplified version of the Lengauer-Tarjan algorithm.
fn compute_immediate_dominators(
    body: &Body,
    predecessors: &HashMap<BasicBlock, Vec<BasicBlock>>,
) -> HashMap<BasicBlock, BasicBlock> {
    let num_blocks = body.basic_blocks.len();
    let entry_node = BasicBlock(0);

    // Iterative dominator computation: idom(n) = LCA of predecessors in the dominator tree.
    let mut idoms: HashMap<BasicBlock, BasicBlock> = HashMap::new();

    let mut changed = true;
    while changed {
        changed = false;

        for i in 1..num_blocks {
            // Skip entry block 0
            let node = BasicBlock(i); // This assumes blocks are 0..N
            let preds = &predecessors[&node];

            if preds.is_empty() {
                continue; // Unreachable block
            }

            // Find first processed predecessor
            let mut new_idom: Option<BasicBlock> = None;
            for &pred in preds {
                if idoms.contains_key(&pred) || pred == entry_node {
                    new_idom = Some(pred);
                    break;
                }
            }

            if let Some(mut candidate) = new_idom {
                for &pred in preds {
                    if pred != candidate && (idoms.contains_key(&pred) || pred == entry_node) {
                        candidate = intersect(&idoms, candidate, pred);
                    }
                }

                if let Some(&current_idom) = idoms.get(&node) {
                    if current_idom != candidate {
                        idoms.insert(node, candidate);
                        changed = true;
                    }
                } else {
                    idoms.insert(node, candidate);
                    changed = true;
                }
            }
        }
    }

    idoms
}

/// Find the Lowest Common Ancestor of two nodes in the dominator tree built so far.
/// Note: This relies on `idoms` being partially populated.
fn intersect(
    idoms: &HashMap<BasicBlock, BasicBlock>,
    b1: BasicBlock,
    b2: BasicBlock,
) -> BasicBlock {
    // Trace ancestors of both nodes to find the first common one.
    let mut visited = HashSet::new();
    let mut curr = b1;
    visited.insert(curr);
    while let Some(&parent) = idoms.get(&curr) {
        curr = parent;
        visited.insert(curr);
        if curr.0 == 0 {
            break;
        } // Reached entry
    }
    // Entry is always visited if we reach it or it was start
    if curr.0 == 0 {
        visited.insert(BasicBlock(0));
    }

    curr = b2;
    while !visited.contains(&curr) {
        if let Some(&parent) = idoms.get(&curr) {
            curr = parent;
        } else {
            // Should be entry if not found, but if we satisfy the precondition
            // that preds are processed, we should convert to entry.
            // But loop condition handles it.
            // If we are here, it means we reached a root that is not in b1's chain?
            // This happens if the graph is disconnected or during early iterations.
            // Assume 0 is common root.
            return BasicBlock(0);
        }
    }

    curr
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
