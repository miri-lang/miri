// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::{
    BasicBlock, Body, Local, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind,
    TerminatorKind,
};
use std::collections::{HashMap, HashSet};

/// Result of SSA construction.
pub struct SSAConstructionResult {
    /// The number of versions created for each original variable.
    pub versions: HashMap<Local, usize>,
}

/// Transform the MIR body into SSA form.
pub fn construct_ssa(body: &mut Body) -> SSAConstructionResult {
    // 1. Compute Dominator Tree (needs immutable body)
    let dom_tree = DominatorTree::compute(body);

    // 2. Initialize Builder (calculates def sites)
    let mut builder = SSABuilder::new(&dom_tree);

    // 3. Run SSA construction (mutates body)
    builder.run(body)
}

struct SSABuilder {
    dom_tree: DominatorTree,
    /// For each variable, list of blocks where it is defined (assigned).
    def_sites: HashMap<Local, HashSet<BasicBlock>>,
    /// For each variable (and original local), mapping to the current SSA version (local index).
    /// Stack of versions for renaming.
    version_stack: HashMap<Local, Vec<Local>>,
    /// Counter for generating new versions.
    version_counts: HashMap<Local, usize>,
    /// Map from new renamed local to original local.
    new_to_old: HashMap<Local, Local>,
}

impl SSABuilder {
    fn new(dom_tree: &DominatorTree) -> Self {
        let new_tree = DominatorTree {
            immediate_dominators: dom_tree.immediate_dominators.clone(),
            dominance_frontiers: dom_tree.dominance_frontiers.clone(),
            children: dom_tree.children.clone(),
        };

        Self {
            dom_tree: new_tree,
            def_sites: HashMap::new(),
            version_stack: HashMap::new(),
            version_counts: HashMap::new(),
            new_to_old: HashMap::new(),
        }
    }

    fn initialize_arguments(&mut self, body: &Body) {
        // For each argument, set current version to itself.
        for i in 1..=body.arg_count {
            let local = Local(i);
            // We consider the original local as the "0-th version".
            self.version_stack.entry(local).or_default().push(local);
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
            versions: self.version_counts.clone(),
        }
    }

    /// Identify which blocks define which variables.
    fn collect_def_sites(&mut self, body: &Body) {
        for (i, block) in body.basic_blocks.iter().enumerate() {
            let bb = BasicBlock(i);
            for stmt in &block.statements {
                if let StatementKind::Assign(place, _) = &stmt.kind {
                    // Only track simple locals for now (no projections)
                    if place.projection.is_empty() {
                        self.def_sites.entry(place.local).or_default().insert(bb);
                    }
                }
            }
            // Terminators can also define variables (call destinations)
            if let Some(terminator) = &block.terminator {
                match &terminator.kind {
                    TerminatorKind::Call { destination, .. }
                    | TerminatorKind::GpuLaunch { destination, .. } => {
                        if destination.projection.is_empty() {
                            self.def_sites
                                .entry(destination.local)
                                .or_default()
                                .insert(bb);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Function parameters are defined in the entry block (implicitly)
        for i in 1..=body.arg_count {
            self.def_sites
                .entry(Local(i))
                .or_default()
                .insert(BasicBlock(0));
        }
    }

    /// Insert trivial Phi nodes at the iterated dominance frontier of definition sites.
    fn insert_phi_nodes(&mut self, body: &mut Body) {
        // For each variable, find the Iterated Dominance Frontier (IDF)
        for (local, defs) in &self.def_sites {
            let mut worklist: Vec<BasicBlock> = defs.iter().cloned().collect();
            let mut processed = HashSet::new();
            let mut has_phi = HashSet::new();

            while let Some(block) = worklist.pop() {
                if let Some(frontier) = self.dom_tree.dominance_frontiers.get(&block) {
                    for &df_node in frontier {
                        if !has_phi.contains(&df_node) {
                            // Insert Phi for 'local' at 'df_node'
                            insert_phi(body, *local, df_node);
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

        // Track which stack entries we added to pop them later
        let mut pushed_counts: HashMap<Local, usize> = HashMap::new();

        // 1. Rename Phis (definitions part) and Statements
        let stmt_count = basic_blocks[block.0].statements.len();
        for i in 0..stmt_count {
            if self.is_phi(basic_blocks, block, i) {
                // LHS is def
                let s = &basic_blocks[block.0].statements[i];
                if let StatementKind::Assign(place, _) = &s.kind {
                    let local = place.local;
                    let new_local = self.new_version(local_decls, local);
                    *pushed_counts.entry(local).or_default() += 1;

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
                }

                // Rewrite Defs
                if let StatementKind::Assign(place, _) = &mut stmt.kind {
                    if place.projection.is_empty() {
                        let new_local = self.new_version(local_decls, place.local);
                        let original = self.get_original_local(place.local);
                        place.local = new_local;
                        *pushed_counts.entry(original).or_default() += 1;
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
                        *pushed_counts.entry(original).or_default() += 1;
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
        for (local, count) in pushed_counts {
            if let Some(stack) = self.version_stack.get_mut(&local) {
                for _ in 0..count {
                    stack.pop();
                }
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
        if let Some(stack) = self.version_stack.get(&original) {
            if let Some(&top) = stack.last() {
                return top;
            }
        }
        local
    }

    fn get_original_local(&self, local: Local) -> Local {
        local
    }

    fn new_version(&mut self, local_decls: &mut Vec<LocalDecl>, original: Local) -> Local {
        let ty = local_decls[original.0].ty.clone();
        let span = local_decls[original.0].span.clone();

        let new_decl = LocalDecl::new(ty, span);
        let new_idx = local_decls.len();
        local_decls.push(new_decl);
        let new_local = Local(new_idx);

        self.version_stack
            .entry(original)
            .or_default()
            .push(new_local);

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
