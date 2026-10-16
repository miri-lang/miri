// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What each statement and terminator does to the facts, resolved once
//! against the [`PlaceTable`] so the dataflow only clears and sets bits.

use std::collections::HashMap;

use super::facts::{FactId, Facts, Mask};
use super::places::PlaceTable;
use super::{allocates, fresh_source, FreshSource, LocalUseCounts};
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    arg_mode, ArgMode, BasicBlock, Body, GpuIntrinsic, Local, Operand, Place, PlaceElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};

/// A mask in [`Program`]'s arena.
#[derive(Debug, Clone, Copy)]
struct MaskId(usize);

/// Whether a store gives its destination a value only it holds.
#[derive(Debug, Clone, Copy)]
enum Freshness {
    Never,
    Always,
    /// When the place moved from is known unshared just before the store.
    IfKnown(FactId),
}

/// A value stored into a place: decide whether it is fresh, forget what the
/// value is read from, forget what the store replaces and every place indexed
/// by the local written, and record the place when the value is fresh.
#[derive(Debug)]
struct Store {
    fresh: Freshness,
    reads: Vec<MaskId>,
    /// What a value that may be shared replaces.
    forget: Option<MaskId>,
    /// What a fresh value replaces: the same, except that a fresh element
    /// spares the other elements of its collection known unshared.
    forget_if_fresh: Option<MaskId>,
    place: Option<FactId>,
}

#[derive(Debug)]
enum Effect {
    Forget(MaskId),
    Store(Store),
}

/// The effects of one block: its statements', then its terminator's.
#[derive(Debug, Default)]
pub(super) struct BlockEffects {
    statements: Vec<Effect>,
    terminator: Vec<Effect>,
}

/// The effects of every block of a body, by block.
pub(super) struct Program {
    blocks: Vec<BlockEffects>,
    masks: Vec<Mask>,
}

impl Program {
    /// Resolves every block of `body`. `asks[block]` tells whether the block
    /// ends with a recognized check, whose call only writes its answer.
    pub(super) fn of(
        body: &Body,
        table: &PlaceTable,
        uses: &LocalUseCounts,
        asks: &[bool],
    ) -> Self {
        let mut builder = ProgramBuilder {
            table,
            uses,
            masks: Vec::new(),
            cache: HashMap::new(),
        };
        let blocks = body
            .basic_blocks
            .iter()
            .enumerate()
            .map(|(index, data)| BlockEffects {
                statements: (data.statements.iter())
                    .filter_map(|statement| builder.statement(statement))
                    .collect(),
                terminator: (data.terminator.as_ref())
                    .and_then(|terminator| {
                        let is_check = asks.get(index).copied().unwrap_or(false);
                        builder.terminator(index, terminator, is_check)
                    })
                    .into_iter()
                    .collect(),
            })
            .collect();
        Self {
            blocks,
            masks: builder.masks,
        }
    }

    /// Applies the statements of `block` to `facts`.
    pub(super) fn apply_statements(&self, facts: &mut Facts, block: usize, table: &PlaceTable) {
        if let Some(effects) = self.blocks.get(block) {
            self.apply(facts, &effects.statements, table);
        }
    }

    /// Applies the terminator of `block` to `facts`.
    pub(super) fn apply_terminator(&self, facts: &mut Facts, block: usize, table: &PlaceTable) {
        if let Some(effects) = self.blocks.get(block) {
            self.apply(facts, &effects.terminator, table);
        }
    }

    fn apply(&self, facts: &mut Facts, effects: &[Effect], table: &PlaceTable) {
        for effect in effects {
            match effect {
                Effect::Forget(mask) => self.forget(facts, *mask),
                Effect::Store(store) => self.store(facts, store, table),
            }
        }
    }

    fn forget(&self, facts: &mut Facts, mask: MaskId) {
        if let Some(mask) = self.masks.get(mask.0) {
            facts.forget(mask);
        }
    }

    fn store(&self, facts: &mut Facts, store: &Store, table: &PlaceTable) {
        let is_fresh = match store.fresh {
            Freshness::Never => false,
            Freshness::Always => true,
            Freshness::IfKnown(source) => facts.contains(source),
        };
        for read in &store.reads {
            self.forget(facts, *read);
        }
        let replaced = if is_fresh {
            store.forget_if_fresh
        } else {
            store.forget
        };
        if let Some(mask) = replaced {
            self.forget(facts, mask);
        }
        if let Some(place) = store.place.filter(|_| is_fresh) {
            table.record_unshared(facts, place);
        }
    }
}

/// What a mask is built for. A read or write reaches the same tracked places
/// whichever locals index it, so masks that do not depend on those locals are
/// keyed by the place with every index spelled alike ([`index_blind`]).
#[derive(PartialEq, Eq, Hash)]
enum MaskKey {
    Reached(Place),
    Replaced(Place),
    ReplacedByFresh(Place),
    Indexed(Local),
}

struct ProgramBuilder<'a> {
    table: &'a PlaceTable,
    uses: &'a LocalUseCounts<'a>,
    masks: Vec<Mask>,
    cache: HashMap<MaskKey, Option<MaskId>>,
}

impl ProgramBuilder<'_> {
    fn statement(&mut self, statement: &Statement) -> Option<Effect> {
        match &statement.kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                let fresh = self.freshness(rvalue);
                let reads = self.rvalue_reads(rvalue);
                self.store(place, fresh, reads)
            }
            StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                self.replaced(place).map(Effect::Forget)
            }
            StatementKind::IncRef(place)
            | StatementKind::DecRef(place)
            | StatementKind::Dealloc(place) => self.reached(place).map(Effect::Forget),
            StatementKind::Nop => None,
        }
    }

    fn terminator(
        &mut self,
        block: usize,
        terminator: &Terminator,
        is_check: bool,
    ) -> Option<Effect> {
        match &terminator.kind {
            TerminatorKind::Call {
                func,
                args,
                arg_modes,
                destination,
                arg_handles: _,
                target: _,
            } => {
                if is_check {
                    return self.store(destination, Freshness::Never, Vec::new());
                }
                self.call(func, args, arg_modes, destination)
            }
            TerminatorKind::VirtualCall {
                args,
                arg_modes,
                destination,
                vtable_slot: _,
                target: _,
            } => {
                let reads = self.argument_reads(args, arg_modes);
                self.store(destination, Freshness::Never, reads)
            }
            TerminatorKind::GpuLaunch { .. } => self.launch(block, terminator),
            TerminatorKind::SwitchInt {
                discr,
                targets: _,
                otherwise: _,
            } => discr
                .place()
                .and_then(|place| self.reached(place))
                .map(Effect::Forget),
            TerminatorKind::Goto { target: _ }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
        }
    }

    /// A call to `func`: it reads its callee and arguments, and stores its
    /// result into `destination`, fresh when `func` allocates it.
    fn call(
        &mut self,
        func: &Operand,
        args: &[Operand],
        arg_modes: &[ArgMode],
        destination: &Place,
    ) -> Option<Effect> {
        let fresh = if allocates(func) {
            Freshness::Always
        } else {
            Freshness::Never
        };
        let mut reads = self.operand_reads([func]);
        reads.extend(self.argument_reads(args, arg_modes));
        self.store(destination, fresh, reads)
    }

    /// A store of a value into `place`; `None` when it cannot change any
    /// fact.
    fn store(&mut self, place: &Place, fresh: Freshness, reads: Vec<MaskId>) -> Option<Effect> {
        let forget = self.replaced(place);
        let forget_if_fresh = match fresh {
            Freshness::Never => forget,
            Freshness::Always | Freshness::IfKnown(_) => self.replaced_by_fresh(place),
        };
        let place = self.table.id(place);
        let records = place.is_some() && !matches!(fresh, Freshness::Never);
        let is_inert =
            reads.is_empty() && forget.is_none() && forget_if_fresh.is_none() && !records;
        (!is_inert).then_some(Effect::Store(Store {
            fresh,
            reads,
            forget,
            forget_if_fresh,
            place,
        }))
    }

    fn freshness(&self, rvalue: &Rvalue) -> Freshness {
        match fresh_source(rvalue) {
            FreshSource::Built => Freshness::Always,
            FreshSource::Moved(source) => match self.table.id(source) {
                Some(id) if self.uses.is_read_once(source.local) => Freshness::IfKnown(id),
                Some(_) | None => Freshness::Never,
            },
            FreshSource::Neither => Freshness::Never,
        }
    }

    /// The facts an rvalue's reads forget.
    fn rvalue_reads(&mut self, rvalue: &Rvalue) -> Vec<MaskId> {
        match rvalue {
            Rvalue::Use(operand) => self.operand_reads([operand]),
            Rvalue::UnaryOp(_, operand) | Rvalue::Cast(operand, _) => {
                self.operand_reads([&**operand])
            }
            Rvalue::BinaryOp(_, left, right) => self.operand_reads([&**left, &**right]),
            Rvalue::Ref(place) => self.reached(place).into_iter().collect(),
            Rvalue::Len(_) => Vec::new(),
            Rvalue::GpuIntrinsic(intrinsic) => self.operand_reads(intrinsic_operand(intrinsic)),
            Rvalue::MathIntrinsic(_, operands) | Rvalue::Aggregate(_, operands) => {
                self.operand_reads(operands)
            }
            Rvalue::AtomicOp {
                buffer,
                index,
                value,
                compare_expected,
                op: _,
            } => {
                let operands = [buffer, index, value].into_iter().chain(compare_expected);
                self.operand_reads(operands.map(|operand| &**operand))
            }
            Rvalue::Phi(incoming) => {
                self.operand_reads(incoming.iter().map(|(operand, _)| operand))
            }
        }
    }

    fn operand_reads<'o>(
        &mut self,
        operands: impl IntoIterator<Item = &'o Operand>,
    ) -> Vec<MaskId> {
        operands
            .into_iter()
            .filter_map(Operand::place)
            .filter_map(|place| self.reached(place))
            .collect()
    }

    /// The facts a call's arguments forget: every place each reaches, and
    /// every place indexed by a local an `out` argument names, since the
    /// callee writes it.
    fn argument_reads(&mut self, args: &[Operand], modes: &[ArgMode]) -> Vec<MaskId> {
        let mut reads = self.operand_reads(args);
        let written = args
            .iter()
            .enumerate()
            .filter(|(position, _)| arg_mode(modes, *position) == ArgMode::Out)
            .filter_map(|(_, arg)| arg.place());
        for place in written {
            reads.extend(self.indexed(place.local));
        }
        reads
    }

    /// Forgets every fact naming a local a kernel launch mentions, as its
    /// root or as an index: the device may write any buffer it is handed.
    fn launch(&mut self, block: usize, terminator: &Terminator) -> Option<Effect> {
        let mut mentioned = MentionedLocals::default();
        mentioned.visit_terminator(BasicBlock(block), terminator);
        let table = self.table;
        let ids = mentioned.0.into_iter().flat_map(|local| {
            let rooted = table.rooted_at(local).map(|(id, _)| id);
            rooted.chain(table.indexed_by(local).iter().copied())
        });
        let mask = Mask::of(ids)?;
        Some(Effect::Forget(self.push(mask)))
    }

    /// The tracked places a read or retain of `reach` reaches.
    fn reached(&mut self, reach: &Place) -> Option<MaskId> {
        self.mask(MaskKey::Reached(index_blind(reach)), |table| {
            table.covered_by(reach).map(|(id, _)| id).collect()
        })
    }

    /// The tracked places a value stored into `place` replaces, with every
    /// place indexed by its local.
    fn replaced(&mut self, place: &Place) -> Option<MaskId> {
        self.mask(MaskKey::Replaced(index_blind(place)), |table| {
            let covered = table.covered_by(place).map(|(id, _)| id);
            covered
                .chain(table.indexed_by(place.local).iter().copied())
                .collect()
        })
    }

    /// As [`Self::replaced`], for a fresh value: storing one element of a
    /// collection leaves its sibling elements known unshared — the elements
    /// stored before it are still held by the collection alone.
    fn replaced_by_fresh(&mut self, place: &Place) -> Option<MaskId> {
        if !matches!(place.projection.last(), Some(PlaceElem::Index(_))) {
            return self.replaced(place);
        }
        self.mask(MaskKey::ReplacedByFresh(place.clone()), |table| {
            let depth = place.projection.len();
            let covered = table
                .covered_by(place)
                .filter(|(_, fact)| fact.projection.len() != depth || *fact == place)
                .map(|(id, _)| id);
            covered
                .chain(table.indexed_by(place.local).iter().copied())
                .collect()
        })
    }

    fn indexed(&mut self, local: Local) -> Option<MaskId> {
        self.mask(MaskKey::Indexed(local), |table| {
            table.indexed_by(local).to_vec()
        })
    }

    fn mask(
        &mut self,
        key: MaskKey,
        ids: impl FnOnce(&PlaceTable) -> Vec<FactId>,
    ) -> Option<MaskId> {
        if let Some(known) = self.cache.get(&key) {
            return *known;
        }
        let mask = Mask::of(ids(self.table)).map(|mask| self.push(mask));
        self.cache.insert(key, mask);
        mask
    }

    fn push(&mut self, mask: Mask) -> MaskId {
        self.masks.push(mask);
        MaskId(self.masks.len() - 1)
    }
}

/// `place` with every index spelled by one local, for keying what depends on
/// a place's shape alone.
fn index_blind(place: &Place) -> Place {
    let projection = place
        .projection
        .iter()
        .map(|elem| match elem {
            PlaceElem::Index(_) => PlaceElem::Index(Local(usize::MAX)),
            PlaceElem::Deref | PlaceElem::Field(_) => elem.clone(),
        })
        .collect();
    Place {
        local: place.local,
        projection,
    }
}

fn intrinsic_operand(intrinsic: &GpuIntrinsic) -> Option<&Operand> {
    match intrinsic {
        GpuIntrinsic::ShuffleDown(operand, _) => Some(operand),
        GpuIntrinsic::ThreadIdx(_)
        | GpuIntrinsic::BlockIdx(_)
        | GpuIntrinsic::BlockDim(_)
        | GpuIntrinsic::GridDim(_)
        | GpuIntrinsic::GlobalIdx(_)
        | GpuIntrinsic::SyncThreads
        | GpuIntrinsic::WarpSize
        | GpuIntrinsic::LaneId => None,
    }
}

/// Every local a visited node mentions, in any role.
#[derive(Default)]
struct MentionedLocals(Vec<Local>);

impl Visitor for MentionedLocals {
    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        self.0.push(local);
    }
}
