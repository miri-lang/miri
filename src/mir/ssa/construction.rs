// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::{
    BasicBlock, Body, Local, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind,
    TerminatorKind,
};
use std::collections::{HashMap, HashSet};

/// Result of SSA construction, containing version counts per local.
pub struct SSAConstructionResult {
    /// The number of versions created for each original variable, indexed by `Local.0`.
    pub versions: Vec<usize>,
}

/// Transform the MIR body into SSA form using iterated dominance frontiers.
///
/// This performs three phases:
/// 1. Collect definition sites for each local
/// 2. Insert Phi nodes at the iterated dominance frontier of each variable's definitions
/// 3. Rename variables via dominator-tree walk, creating fresh versions at each definition
///
/// # Arguments
///
/// * `body` - The MIR function body to transform (mutated in place)
///
/// # Returns
///
/// An [`SSAConstructionResult`] containing version counts per original local.
pub fn construct_ssa(body: &mut Body) -> SSAConstructionResult {
    // 1. Compute Dominator Tree (needs immutable body)
    let dom_tree = DominatorTree::compute(body);

    // 2. Initialize Builder (calculates def sites)
    let num_locals = body.local_decls.len();
    let mut builder = SSABuilder::new(dom_tree, num_locals);

    // 3. Run SSA construction (mutates body)
    builder.run(body)
}

struct SSABuilder {
    dom_tree: DominatorTree,
    /// For each original local, the set of blocks where it is defined. Indexed by `Local.0`.
    def_sites: Vec<HashSet<BasicBlock>>,
    /// Stack of SSA versions for each original local during renaming. Indexed by `Local.0`.
    version_stack: Vec<Vec<Local>>,
    /// Counter for generating new versions per original local. Indexed by `Local.0`.
    version_counts: Vec<usize>,
    /// Map from new renamed local to original local (sparse, only for SSA-created locals).
    new_to_old: HashMap<Local, Local>,
}

impl SSABuilder {
    fn new(dom_tree: DominatorTree, num_locals: usize) -> Self {
        Self {
            dom_tree,
            def_sites: vec![HashSet::new(); num_locals],
            version_stack: vec![Vec::new(); num_locals],
            version_counts: vec![0; num_locals],
            new_to_old: HashMap::new(),
        }
    }

    fn initialize_arguments(&mut self, body: &Body) {
        // For each argument, set current version to itself.
        for i in 1..=body.arg_count {
            let local = Local(i);
            self.version_stack[local.0].push(local);
        }
    }

    fn is_original(&self, local: Local) -> bool {
        !self.new_to_old.contains_key(&local)
    }

    fn run(&mut self, body: &mut Body) -> SSAConstructionResult {
        // 1. Collect definition sites
        self.collect_def_sites(body);

        // 2. Insert Phi nodes
        self.insert_phi_nodes(body);

        // 3. Rename variables
        self.initialize_arguments(body);
        self.rename_variables(body, BasicBlock(0));

        SSAConstructionResult {
            versions: std::mem::take(&mut self.version_counts),
        }
    }

    /// Identify which blocks define which variables.
    fn collect_def_sites(&mut self, body: &Body) {
        for (i, block) in body.basic_blocks.iter().enumerate() {
            let bb = BasicBlock(i);
            for stmt in &block.statements {
                if let StatementKind::Assign(place, _) = &stmt.kind {
                    if place.projection.is_empty() {
                        self.def_sites[place.local.0].insert(bb);
                    }
                }
            }
            if let Some(terminator) = &block.terminator {
                match &terminator.kind {
                    TerminatorKind::Call { destination, .. }
                    | TerminatorKind::GpuLaunch { destination, .. } => {
                        if destination.projection.is_empty() {
                            self.def_sites[destination.local.0].insert(bb);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Function parameters are defined in the entry block (implicitly)
        for i in 1..=body.arg_count {
            self.def_sites[i].insert(BasicBlock(0));
        }
    }

    /// Insert trivial Phi nodes at the iterated dominance frontier of definition sites.
    fn insert_phi_nodes(&mut self, body: &mut Body) {
        for (idx, defs) in self.def_sites.iter().enumerate() {
            if defs.is_empty() {
                continue;
            }
            let local = Local(idx);
            let mut worklist: Vec<BasicBlock> = defs.iter().copied().collect();
            let mut processed = HashSet::new();
            let mut has_phi = HashSet::new();

            while let Some(block) = worklist.pop() {
                if let Some(frontier) = self.dom_tree.dominance_frontiers.get(&block) {
                    for &df_node in frontier {
                        if !has_phi.contains(&df_node) {
                            insert_phi(body, local, df_node);
                            has_phi.insert(df_node);

                            if !processed.contains(&df_node) {
                                worklist.push(df_node);
                                processed.insert(df_node);
                            }
                        }
                    }
                }
            }
        }
    }

    fn rename_variables(&mut self, body: &mut Body, block: BasicBlock) {
        let (basic_blocks, local_decls) = (&mut body.basic_blocks, &mut body.local_decls);

        // Track which stack entries we added per original local, to pop them later.
        // Sized for original locals only; new SSA locals always map back to an original.
        let num_originals = self.version_stack.len();
        let mut pushed_counts: Vec<usize> = vec![0; num_originals];

        // 1. Rename Phis (definitions part) and Statements
        let stmt_count = basic_blocks[block.0].statements.len();
        for i in 0..stmt_count {
            if self.is_phi(basic_blocks, block, i) {
                // LHS is def
                let s = &basic_blocks[block.0].statements[i];
                if let StatementKind::Assign(place, _) = &s.kind {
                    let local = place.local;
                    let new_local = self.new_version(local_decls, local);
                    pushed_counts[local.0] += 1;

                    // Mutate
                    if let StatementKind::Assign(p, _) =
                        &mut basic_blocks[block.0].statements[i].kind
                    {
                        p.local = new_local;
                    }
                }
            } else {
                // Regular statement
                let stmt = &mut basic_blocks[block.0].statements[i];

                // Rewrite Uses first
                match &mut stmt.kind {
                    StatementKind::Assign(_, rvalue) => {
                        self.rewrite_uses_in_rvalue(rvalue);
                    }
                    StatementKind::Nop
                    | StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_) => {}
                    StatementKind::IncRef(place)
                    | StatementKind::DecRef(place)
                    | StatementKind::Dealloc(place) => {
                        if place.projection.is_empty() {
                            place.local = self.get_current_version(place.local);
                        }
                    }
                }

                // Rewrite Defs
                if let StatementKind::Assign(place, _) = &mut stmt.kind {
                    if place.projection.is_empty() {
                        let new_local = self.new_version(local_decls, place.local);
                        let original = self.get_original_local(place.local);
                        place.local = new_local;
                        pushed_counts[original.0] += 1;
                    }
                }
            }
        }

        // Rewrite Terminator uses
        if let Some(terminator) = &mut basic_blocks[block.0].terminator {
            self.rewrite_uses_in_terminator(terminator);
        }

        // Rewrite Defs in Terminator
        if let Some(terminator) = &mut basic_blocks[block.0].terminator {
            match &mut terminator.kind {
                TerminatorKind::Call { destination, .. }
                | TerminatorKind::GpuLaunch { destination, .. } => {
                    if destination.projection.is_empty() {
                        let new_local = self.new_version(local_decls, destination.local);
                        let original = self.get_original_local(destination.local);
                        destination.local = new_local;
                        pushed_counts[original.0] += 1;
                    }
                }
                _ => {}
            }
        }

        // 2. Fill Phi arguments in successors
        let successors = self.get_successors(basic_blocks, block);
        for succ in successors {
            let succ_indices: Vec<usize> = (0..basic_blocks[succ.0].statements.len())
                .filter(|&idx| self.is_phi(basic_blocks, succ, idx))
                .collect();

            for idx in succ_indices {
                let stmt = &mut basic_blocks[succ.0].statements[idx];
                if let StatementKind::Assign(place, Rvalue::Phi(args)) = &mut stmt.kind {
                    // Determine original local
                    let phi_original_local = if self.is_original(place.local) {
                        place.local
                    } else {
                        // Safe: if not original, it must be in new_to_old
                        self.new_to_old
                            .get(&place.local)
                            .copied()
                            .unwrap_or(place.local)
                    };

                    // Get current version
                    let current_val = self.get_current_version(phi_original_local);

                    // Add to Phi args
                    args.push((Operand::Copy(Place::new(current_val)), block));
                }
            }
        }

        // 3. Recurse into children in DomTree
        // To avoid borrow issues, we collect children first.
        let children = self
            .dom_tree
            .children
            .get(&block)
            .cloned()
            .unwrap_or_default();
        for child in children {
            self.rename_variables(body, child);
        }

        // 4. Pop stack
        for (idx, &count) in pushed_counts.iter().enumerate() {
            if count > 0 {
                let stack = &mut self.version_stack[idx];
                stack.truncate(stack.len().saturating_sub(count));
            }
        }
    }

    // Helpers...
    fn is_phi(
        &self,
        blocks: &[crate::mir::block::BasicBlockData],
        block: BasicBlock,
        stmt_idx: usize,
    ) -> bool {
        matches!(
            &blocks[block.0].statements[stmt_idx].kind,
            StatementKind::Assign(_, Rvalue::Phi(_))
        )
    }

    fn rewrite_uses_in_terminator(&mut self, terminator: &mut crate::mir::Terminator) {
        match &mut terminator.kind {
            TerminatorKind::SwitchInt { discr, .. } => self.rewrite_operand(discr),
            TerminatorKind::Call { func, args, .. } => {
                self.rewrite_operand(func);
                for arg in args {
                    self.rewrite_operand(arg);
                }
            }
            TerminatorKind::GpuLaunch {
                kernel,
                grid,
                block: grid_block,
                ..
            } => {
                self.rewrite_operand(kernel);
                self.rewrite_operand(grid);
                self.rewrite_operand(grid_block);
            }
            TerminatorKind::Return | TerminatorKind::Goto { .. } | TerminatorKind::Unreachable => {}
        }
    }

    fn rewrite_uses_in_rvalue(&mut self, rvalue: &mut Rvalue) {
        match rvalue {
            Rvalue::Use(op) => self.rewrite_operand(op),
            Rvalue::UnaryOp(_, op) | Rvalue::Cast(op, _) => self.rewrite_operand(op),
            Rvalue::BinaryOp(_, lhs, rhs) => {
                self.rewrite_operand(lhs);
                self.rewrite_operand(rhs);
            }
            Rvalue::Aggregate(_, ops) => {
                for op in ops {
                    self.rewrite_operand(op);
                }
            }
            Rvalue::Ref(place) | Rvalue::Len(place) => {
                if place.projection.is_empty() {
                    place.local = self.get_current_version(place.local);
                }
            }
            Rvalue::Phi(_) => {
                // Phis are definitions in this block
            }
            Rvalue::GpuIntrinsic(_) => {}
            Rvalue::Allocate(size, align, alloc) => {
                self.rewrite_operand(size);
                self.rewrite_operand(align);
                self.rewrite_operand(alloc);
            }
        }
    }

    fn rewrite_operand(&mut self, op: &mut Operand) {
        match op {
            Operand::Copy(place) | Operand::Move(place) => {
                if place.projection.is_empty() {
                    place.local = self.get_current_version(place.local);
                }
            }
            Operand::Constant(_) => {}
        }
    }

    fn get_current_version(&self, local: Local) -> Local {
        let original = self.get_original_local(local);
        if original.0 < self.version_stack.len() {
            if let Some(&top) = self.version_stack[original.0].last() {
                return top;
            }
        }
        local
    }

    /// Resolve a potentially-renamed local back to its original pre-SSA local.
    fn get_original_local(&self, local: Local) -> Local {
        self.new_to_old.get(&local).copied().unwrap_or(local)
    }

    fn new_version(&mut self, local_decls: &mut Vec<LocalDecl>, original: Local) -> Local {
        let ty = local_decls[original.0].ty.clone();
        let span = local_decls[original.0].span;

        let new_decl = LocalDecl::new(ty, span);
        let new_idx = local_decls.len();
        local_decls.push(new_decl);
        let new_local = Local(new_idx);

        self.version_stack[original.0].push(new_local);
        self.version_counts[original.0] += 1;
        self.new_to_old.insert(new_local, original);

        new_local
    }

    fn get_successors(
        &self,
        basic_blocks: &[crate::mir::block::BasicBlockData],
        block: BasicBlock,
    ) -> Vec<BasicBlock> {
        let mut succs = Vec::new();
        if let Some(term) = &basic_blocks[block.0].terminator {
            match &term.kind {
                TerminatorKind::Goto { target } => succs.push(*target),
                TerminatorKind::SwitchInt {
                    targets, otherwise, ..
                } => {
                    for (_, t) in targets {
                        succs.push(*t);
                    }
                    succs.push(*otherwise);
                }
                TerminatorKind::Call { target, .. } | TerminatorKind::GpuLaunch { target, .. } => {
                    if let Some(t) = target {
                        succs.push(*t);
                    }
                }
                TerminatorKind::Return | TerminatorKind::Unreachable => {}
            }
        }
        succs
    }
}

fn insert_phi(body: &mut Body, local: Local, block: BasicBlock) {
    // We don't know the operands yet, they will be filled during renaming.
    // For now, empty Phi.
    let phi = Rvalue::Phi(Vec::new());

    // We insert `local = Phi(...)` and then rename `local` to `local_vN`.
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(local), phi),
        span: Default::default(),
    };

    body.basic_blocks[block.0].statements.insert(0, stmt);
}
