// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Holding promoted cells as SSA values.
//!
//! Each cell's liveness is found first, block by block. A block with more
//! than one predecessor takes one parameter for each cell live into it, and
//! a block with one takes its predecessor's values. The blocks are then
//! rewritten in reverse postorder: a write of a cell becomes the cell's
//! current value and a read becomes an alias of it, converted by a bitcast
//! when the access's type is not the cell's. Last, every branch hands its
//! target the values the target's new parameters stand for.

use super::cells::{Cells, Direction};
use crate::error::CodegenError;
use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::entity::SecondaryMap;
use cranelift_codegen::flowgraph::ControlFlowGraph;
use cranelift_codegen::ir::immediates::{Ieee32, Ieee64};
use cranelift_codegen::ir::{
    types, Block, BlockArg, BlockCall, Function, Inst, InstBuilder, InstructionData, JumpTable,
    JumpTableData, MemFlagsData, Opcode, Type, Value, ValueDef,
};

/// A set of cells, by index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CellSet {
    words: Vec<u64>,
}

impl CellSet {
    fn with_capacity(cells: usize) -> Self {
        Self {
            words: vec![0; cells.div_ceil(64)],
        }
    }

    fn insert(&mut self, cell: usize) {
        if let Some(word) = self.words.get_mut(cell / 64) {
            *word |= 1 << (cell % 64);
        }
    }

    fn contains(&self, cell: usize) -> bool {
        (self.words.get(cell / 64)).is_some_and(|word| word & (1 << (cell % 64)) != 0)
    }

    /// The cells, in increasing order.
    pub(super) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (self.words.iter().enumerate()).flat_map(|(index, &word)| {
            (0..64)
                .filter(move |bit| word & (1 << bit) != 0)
                .map(move |bit| index * 64 + bit)
        })
    }

    /// Add every cell of `other`.
    fn add(&mut self, other: &Self) {
        for (word, &added) in self.words.iter_mut().zip(&other.words) {
            *word |= added;
        }
    }

    /// The cells of `generated` and those of `self` not in `killed`.
    fn through(&self, generated: &Self, killed: &Self) -> Self {
        let words = (self.words.iter().zip(&generated.words).zip(&killed.words))
            .map(|((&live, &generated), &killed)| generated | (live & !killed))
            .collect();
        Self { words }
    }
}

/// Which cells are live into and out of each block.
pub(super) struct Liveness {
    live_in: SecondaryMap<Block, CellSet>,
    live_out: SecondaryMap<Block, CellSet>,
}

impl Liveness {
    /// The cells live into `block`.
    pub(super) fn live_in(&self, block: Block) -> &CellSet {
        &self.live_in[block]
    }
}

/// The cells read before they are written in a block, and those written
/// in it.
#[derive(Clone)]
struct BlockEffect {
    generated: CellSet,
    killed: CellSet,
}

/// The liveness of `cells` in `function`, whose reachable blocks are
/// `order` in reverse postorder.
pub(super) fn liveness(
    function: &Function,
    cfg: &ControlFlowGraph,
    order: &[Block],
    cells: &Cells,
) -> Liveness {
    let count = cells.cells.len();
    let mut effects: SecondaryMap<Block, Option<BlockEffect>> = SecondaryMap::new();
    for &block in order {
        effects[block] = Some(block_effect(function, block, cells));
    }
    let empty = CellSet::with_capacity(count);
    let mut liveness = Liveness {
        live_in: SecondaryMap::with_default(empty.clone()),
        live_out: SecondaryMap::with_default(empty.clone()),
    };
    let mut changed = true;
    while changed {
        changed = false;
        for &block in order.iter().rev() {
            let mut out = empty.clone();
            for successor in cfg.succ_iter(block) {
                out.add(&liveness.live_in[successor]);
            }
            let Some(effect) = &effects[block] else {
                continue;
            };
            let live_in = out.through(&effect.generated, &effect.killed);
            changed |= live_in != liveness.live_in[block];
            liveness.live_in[block] = live_in;
            liveness.live_out[block] = out;
        }
    }
    liveness
}

/// What `block` of `function` does to `cells`.
fn block_effect(function: &Function, block: Block, cells: &Cells) -> BlockEffect {
    let count = cells.cells.len();
    let mut effect = BlockEffect {
        generated: CellSet::with_capacity(count),
        killed: CellSet::with_capacity(count),
    };
    for inst in function.layout.block_insts(block) {
        let Some(access) = cells.accesses[inst] else {
            continue;
        };
        match access.direction {
            Direction::Read if !effect.killed.contains(access.cell) => {
                effect.generated.insert(access.cell);
            }
            Direction::Read => {}
            Direction::Write => effect.killed.insert(access.cell),
        }
    }
    effect
}

/// Hold `cells` of `function`, whose reachable blocks are `order` in reverse
/// postorder, as SSA values, removing every access of them.
pub(super) fn rewrite(
    function: &mut Function,
    cfg: &ControlFlowGraph,
    order: &[Block],
    cells: &Cells,
    liveness: &Liveness,
) -> Result<(), CodegenError> {
    let mut rewrite = Rewrite {
        cells,
        liveness,
        current: vec![None; cells.cells.len()],
        entering: SecondaryMap::new(),
        leaving: SecondaryMap::new(),
        conversions: Vec::new(),
        insts: Vec::new(),
    };
    rewrite.add_parameters(function, cfg, order);
    for &block in order {
        rewrite.rewrite_block(function, cfg, block)?;
    }
    rewrite.hand_values(function, order)?;
    rewrite.remove_unused_conversions(function, order);
    Ok(())
}

/// The state rewriting one function keeps.
struct Rewrite<'a> {
    cells: &'a Cells,
    liveness: &'a Liveness,
    /// Each cell's value at the point being rewritten.
    current: Vec<Option<Value>>,
    /// The parameters added to each block, by the cell each stands for.
    entering: SecondaryMap<Block, Vec<(usize, Value)>>,
    /// Each block's values of the cells live out of it, by cell.
    leaving: SecondaryMap<Block, Vec<(usize, Option<Value>)>>,
    /// Every conversion added, in the order it was added.
    conversions: Vec<Inst>,
    /// The instructions of the block being rewritten.
    insts: Vec<Inst>,
}

impl Rewrite<'_> {
    /// Give each block with more than one predecessor a parameter for each
    /// cell live into it.
    fn add_parameters(&mut self, function: &mut Function, cfg: &ControlFlowGraph, order: &[Block]) {
        for &block in order.iter().skip(1) {
            if sole_predecessor(cfg, block).is_some() {
                continue;
            }
            let parameters = (self.liveness.live_in(block).iter())
                .map(|cell| {
                    let ty = self.cells.cells[cell].ty;
                    (cell, function.dfg.append_block_param(block, ty))
                })
                .collect();
            self.entering[block] = parameters;
        }
    }

    /// Rewrite every access of a cell in `block`.
    fn rewrite_block(
        &mut self,
        function: &mut Function,
        cfg: &ControlFlowGraph,
        block: Block,
    ) -> Result<(), CodegenError> {
        self.enter(cfg, block);
        self.insts.clear();
        self.insts.extend(function.layout.block_insts(block));
        for index in 0..self.insts.len() {
            let inst = self.insts[index];
            let Some(access) = self.cells.accesses[inst] else {
                continue;
            };
            match access.direction {
                Direction::Write => self.write(function, inst, access.cell)?,
                Direction::Read => self.read(function, inst, access.cell)?,
            }
        }
        self.leaving[block] = (self.liveness.live_out[block].iter())
            .map(|cell| (cell, self.current[cell]))
            .collect();
        Ok(())
    }

    /// Set each cell's current value to the one it has entering `block`.
    fn enter(&mut self, cfg: &ControlFlowGraph, block: Block) {
        self.current.fill(None);
        let entering = match sole_predecessor(cfg, block) {
            Some(predecessor) => &self.leaving[predecessor],
            None => {
                for &(cell, value) in &self.entering[block] {
                    self.current[cell] = Some(value);
                }
                return;
            }
        };
        for &(cell, value) in entering {
            self.current[cell] = value;
        }
    }

    /// Make the value the store `inst` writes the current value of `cell`,
    /// and remove the store.
    fn write(
        &mut self,
        function: &mut Function,
        inst: Inst,
        cell: usize,
    ) -> Result<(), CodegenError> {
        let written = match function.dfg.insts[inst] {
            InstructionData::Store {
                args: [written, _], ..
            } => written,
            _ => {
                return Err(unexpected(
                    "a cell is written by an instruction that stores nothing",
                ))
            }
        };
        let written = function.dfg.resolve_aliases(written);
        let value = self.convert(function, inst, written, self.cells.cells[cell].ty);
        self.current[cell] = Some(value);
        function.layout.remove_inst(inst);
        Ok(())
    }

    /// Make the result of the load `inst` an alias of the current value of
    /// `cell`, and remove the load.
    fn read(
        &mut self,
        function: &mut Function,
        inst: Inst,
        cell: usize,
    ) -> Result<(), CodegenError> {
        let result = (function.dfg.inst_results(inst).first().copied())
            .ok_or_else(|| unexpected("a cell is read by an instruction with no result"))?;
        let current = self.current[cell]
            .ok_or_else(|| unexpected("a cell is read where it holds no value"))?;
        let ty = function.dfg.value_type(result);
        let value = self.convert(function, inst, current, ty);
        function.dfg.clear_results(inst);
        function.dfg.change_to_alias(result, value);
        function.layout.remove_inst(inst);
        Ok(())
    }

    /// `value` as one of type `ty`, any instruction converting it inserted
    /// before `at` with its source location.
    fn convert(&mut self, function: &mut Function, at: Inst, value: Value, ty: Type) -> Value {
        if function.dfg.value_type(value) == ty {
            return value;
        }
        if let Some(original) = bitcast_source(function, value)
            .filter(|&original| function.dfg.value_type(original) == ty)
        {
            return original;
        }
        let constant = constant_bits(function, value);
        let srcloc = function.srcloc(at);
        let mut cursor = FuncCursor::new(function).at_inst(at);
        if !srcloc.is_default() {
            cursor.set_srcloc(srcloc);
        }
        let converted = match (constant, ty) {
            (Some(bits), types::F64) => cursor.ins().f64const(Ieee64::with_bits(bits)),
            (Some(bits), types::F32) => match u32::try_from(bits) {
                Ok(bits) => cursor.ins().f32const(Ieee32::with_bits(bits)),
                Err(_) => cursor.ins().bitcast(ty, MemFlagsData::new(), value),
            },
            _ => cursor.ins().bitcast(ty, MemFlagsData::new(), value),
        };
        if let ValueDef::Result(inst, _) = function.dfg.value_def(converted) {
            self.conversions.push(inst);
        }
        converted
    }

    /// Hand every block that took parameters, along each branch to it, the
    /// values its parameters stand for there.
    fn hand_values(&self, function: &mut Function, order: &[Block]) -> Result<(), CodegenError> {
        let mut tables = table_uses(function, order);
        for &block in order {
            let Some(branch) = function.layout.last_inst(block) else {
                continue;
            };
            unshare_table(function, branch, &mut tables);
            let dfg = &mut function.dfg;
            let leaving = &self.leaving[block];
            let calls = dfg.insts[branch]
                .branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables);
            for call in calls {
                let target = call.block(&dfg.value_lists);
                let entering = &self.entering[target];
                if entering.is_empty() {
                    continue;
                }
                let arguments: Vec<BlockArg> = call.args(&dfg.value_lists).collect();
                let mut handed = BlockCall::new(target, arguments, &mut dfg.value_lists);
                for &(cell, _) in entering {
                    handed.append_argument(value_leaving(leaving, cell)?, &mut dfg.value_lists);
                }
                *call = handed;
            }
        }
        Ok(())
    }

    /// Remove every conversion added that nothing uses.
    fn remove_unused_conversions(&self, function: &mut Function, order: &[Block]) {
        let mut uses: SecondaryMap<Value, u32> = SecondaryMap::new();
        for &block in order {
            for inst in function.layout.block_insts(block) {
                for value in function.dfg.inst_values(inst) {
                    uses[function.dfg.resolve_aliases(value)] += 1;
                }
            }
        }
        for &inst in self.conversions.iter().rev() {
            let unused = (function.dfg.inst_results(inst).iter()).all(|&result| uses[result] == 0);
            if !unused {
                continue;
            }
            for &argument in function.dfg.inst_args(inst) {
                let used = &mut uses[function.dfg.resolve_aliases(argument)];
                *used = used.saturating_sub(1);
            }
            function.layout.remove_inst(inst);
        }
    }
}

/// The one block every branch to `block` is from, if there is one.
fn sole_predecessor(cfg: &ControlFlowGraph, block: Block) -> Option<Block> {
    let mut predecessors = cfg.pred_iter(block).map(|predecessor| predecessor.block);
    let first = predecessors.next()?;
    predecessors.all(|other| other == first).then_some(first)
}

/// The value a `bitcast` defining `value` converts.
fn bitcast_source(function: &Function, value: Value) -> Option<Value> {
    let ValueDef::Result(inst, _) = function.dfg.value_def(value) else {
        return None;
    };
    match function.dfg.insts[inst] {
        InstructionData::LoadNoOffset {
            opcode: Opcode::Bitcast,
            arg,
            ..
        } => Some(arg),
        _ => None,
    }
}

/// The bits of the integer constant `value` is defined as.
fn constant_bits(function: &Function, value: Value) -> Option<u64> {
    let ValueDef::Result(inst, _) = function.dfg.value_def(value) else {
        return None;
    };
    match function.dfg.insts[inst] {
        InstructionData::UnaryImm {
            opcode: Opcode::Iconst,
            imm,
        } => {
            let width = function.dfg.value_type(value).bits();
            let bits = imm.bits().cast_unsigned();
            Some(if width < 64 {
                bits & ((1 << width) - 1)
            } else {
                bits
            })
        }
        _ => None,
    }
}

/// How many of the branches ending `order`'s blocks use each jump table.
fn table_uses(function: &Function, order: &[Block]) -> SecondaryMap<JumpTable, u32> {
    let mut uses: SecondaryMap<JumpTable, u32> = SecondaryMap::new();
    for &block in order {
        let branch = function.layout.last_inst(block);
        if let Some(InstructionData::BranchTable { table, .. }) =
            branch.map(|inst| function.dfg.insts[inst])
        {
            uses[table] += 1;
        }
    }
    uses
}

/// Give `branch`, when it is a `br_table` sharing its jump table with
/// another branch, a copy of the table of its own, so the values it hands
/// are its alone.
fn unshare_table(function: &mut Function, branch: Inst, uses: &mut SecondaryMap<JumpTable, u32>) {
    let InstructionData::BranchTable { table, .. } = function.dfg.insts[branch] else {
        return;
    };
    if uses[table] <= 1 {
        return;
    }
    uses[table] -= 1;
    let dfg = &mut function.dfg;
    let copies: Vec<BlockCall> = (dfg.jump_tables[table].all_branches().to_vec().into_iter())
        .map(|call| {
            let arguments: Vec<BlockArg> = call.args(&dfg.value_lists).collect();
            BlockCall::new(
                call.block(&dfg.value_lists),
                arguments,
                &mut dfg.value_lists,
            )
        })
        .collect();
    let Some((default, entries)) = copies.split_first() else {
        return;
    };
    let copy = dfg.jump_tables.push(JumpTableData::new(*default, entries));
    if let InstructionData::BranchTable { table, .. } = &mut dfg.insts[branch] {
        *table = copy;
    }
}

/// The value `cell` has in `leaving`, a block's values of the cells live out
/// of it sorted by cell.
fn value_leaving(leaving: &[(usize, Option<Value>)], cell: usize) -> Result<Value, CodegenError> {
    let index = (leaving.binary_search_by_key(&cell, |&(leaving_cell, _)| leaving_cell))
        .map_err(|_| unexpected("a cell live into a block is not live out of its predecessor"))?;
    leaving[index]
        .1
        .ok_or_else(|| unexpected("a cell live out of a block holds no value there"))
}

/// The error a broken invariant of the rewrite reports.
fn unexpected(details: &str) -> CodegenError {
    CodegenError::Internal(format!("slot promotion: {details}"))
}
