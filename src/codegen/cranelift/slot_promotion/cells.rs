// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The cells of a function's promotable stack slots: which runs of bytes
//! each slot is read and written in, and which instructions do it.
//!
//! An address in a slot is a `stack_addr` of it, or such an address plus a
//! constant by `iadd` with an `iconst`. A slot is promotable when every use
//! of every address in it is one of those additions or the address operand
//! of a plain `load` or `store` of native byte order, and when every access
//! of it lies within the slot and either covers exactly the bytes of every
//! access it overlaps or overlaps none. Cranelift's `stack_load` and
//! `stack_store` builders emit a `stack_addr` and a plain access, so those
//! are found the same way.

use cranelift_codegen::entity::SecondaryMap;
use cranelift_codegen::ir::{
    types, Block, Function, Inst, InstructionData, MemFlagsData, Opcode, StackSlot, StackSlotKind,
    Type, Value, ValueDef,
};

/// A run of a slot's bytes that every access reads or writes whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) slot: StackSlot,
    pub(super) offset: u32,
    /// The type the cell's SSA values have.
    pub(super) ty: Type,
}

/// Whether an access reads its cell or writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Read,
    Write,
}

/// One instruction's access of a cell, by its index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Access {
    pub(super) cell: usize,
    pub(super) direction: Direction,
}

/// The cells of every promotable slot of a function, sorted by slot and
/// offset, and the instructions that reach them.
#[derive(Default)]
pub(super) struct Cells {
    pub(super) cells: Vec<Cell>,
    /// The access each load or store of a cell makes.
    pub(super) accesses: SecondaryMap<Inst, Option<Access>>,
    /// Every promotable slot, with each instruction computing an address in
    /// it.
    slots: Vec<(StackSlot, Vec<Inst>)>,
}

impl Cells {
    /// The promotable slots, each with the instructions computing an
    /// address in it.
    pub(super) fn slots(&self) -> &[(StackSlot, Vec<Inst>)] {
        &self.slots
    }

    /// These cells less those of the slots `is_kept` holds for.
    pub(super) fn without(self, is_kept: impl Fn(StackSlot) -> bool) -> Self {
        let mut renumbered: Vec<Option<usize>> = Vec::with_capacity(self.cells.len());
        let mut cells = Vec::with_capacity(self.cells.len());
        for cell in self.cells {
            renumbered.push((!is_kept(cell.slot)).then_some(cells.len()));
            if !is_kept(cell.slot) {
                cells.push(cell);
            }
        }
        let mut accesses = self.accesses;
        for (_, access) in accesses.iter_mut() {
            *access = access.and_then(|kept| {
                let cell = renumbered.get(kept.cell).copied().flatten()?;
                Some(Access { cell, ..kept })
            });
        }
        let slots = (self.slots.into_iter())
            .filter(|(slot, _)| !is_kept(*slot))
            .collect();
        Self {
            cells,
            accesses,
            slots,
        }
    }
}

/// The cells of `function`'s promotable slots, its blocks visited in
/// `order`, a reverse postorder of the reachable ones, in which every value
/// is defined before it is used.
pub(super) fn discover(function: &Function, order: &[Block]) -> Cells {
    let addresses = addresses_in(function, order);
    let mut survey = Survey::new(function);
    for &block in order {
        for inst in function.layout.block_insts(block) {
            survey.visit(function, &addresses, inst);
        }
    }
    survey.into_cells(function)
}

/// A byte of a slot, as an address names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Address {
    slot: StackSlot,
    offset: i64,
}

impl Address {
    /// This address moved by `delta` bytes, when the offset can be held.
    fn moved(self, delta: i64) -> Option<Self> {
        Some(Self {
            offset: self.offset.checked_add(delta)?,
            ..self
        })
    }
}

/// The addresses in a slot that each value of `function` holds, and the
/// instruction computing each, its blocks visited in `order`.
pub(super) fn addresses_in(
    function: &Function,
    order: &[Block],
) -> SecondaryMap<Value, Option<Address>> {
    let mut addresses: SecondaryMap<Value, Option<Address>> = SecondaryMap::new();
    for &block in order {
        for inst in function.layout.block_insts(block) {
            let computed = address_computed(function, &addresses, inst);
            if let Some((address, result)) = computed.zip(result_of(function, inst)) {
                addresses[result] = Some(address);
            }
        }
    }
    addresses
}

/// The address in a slot `inst` computes, given the `addresses` its
/// operands hold.
fn address_computed(
    function: &Function,
    addresses: &SecondaryMap<Value, Option<Address>>,
    inst: Inst,
) -> Option<Address> {
    if let Some(address) = stack_addr_of(function, inst) {
        return Some(address);
    }
    match function.dfg.insts[inst] {
        InstructionData::Binary {
            opcode: Opcode::Iadd,
            args: [left, right],
        } => match (addresses[left], addresses[right]) {
            (Some(address), None) => address.moved(constant(function, right)?),
            (None, Some(address)) => address.moved(constant(function, left)?),
            (Some(_), Some(_)) | (None, None) => None,
        },
        _ => None,
    }
}

/// Whether `slot` of `function` is an explicit slot.
fn is_explicit(function: &Function, slot: StackSlot) -> bool {
    (function.sized_stack_slots.get(slot))
        .is_some_and(|data| data.kind == StackSlotKind::ExplicitSlot)
}

/// The constant `value` is defined as by an `iconst`, sign-extended from
/// the width of its type.
pub(in crate::codegen::cranelift) fn constant(function: &Function, value: Value) -> Option<i64> {
    let ValueDef::Result(inst, _) = function.dfg.value_def(value) else {
        return None;
    };
    match function.dfg.insts[inst] {
        InstructionData::UnaryImm {
            opcode: Opcode::Iconst,
            imm,
        } => {
            // An `iconst` keeps its immediate masked to its type's width, so
            // a negative offset of a narrower type reads back sign-extended.
            let width = function.dfg.value_type(value).bits();
            Some(imm.sign_extend_from_width(width).bits())
        }
        _ => None,
    }
}

/// One access of a slot as the survey meets it.
#[derive(Debug, Clone, Copy)]
struct SlotAccess {
    inst: Inst,
    offset: i64,
    ty: Type,
    direction: Direction,
}

impl SlotAccess {
    /// The bytes the access covers, from its offset.
    fn bytes(&self) -> (i64, i64) {
        (self.offset, i64::from(self.ty.bytes()))
    }
}

/// What one instruction does with an address in a slot.
enum Role {
    /// Accesses the slot through the address operand.
    Accesses(StackSlot, SlotAccess, Value),
    /// Adds a constant to the address operand, an address in the slot.
    Offsets(Value, StackSlot),
    Other,
}

/// Every slot's accesses and whether its address escapes, gathered one
/// instruction at a time.
struct Survey {
    accesses: SecondaryMap<StackSlot, Vec<SlotAccess>>,
    escaped: SecondaryMap<StackSlot, bool>,
    computed: SecondaryMap<StackSlot, Vec<Inst>>,
}

impl Survey {
    fn new(function: &Function) -> Self {
        let slots = function.sized_stack_slots.len();
        Self {
            accesses: SecondaryMap::with_capacity(slots),
            escaped: SecondaryMap::with_capacity(slots),
            computed: SecondaryMap::with_capacity(slots),
        }
    }

    /// Record what `inst` does with the slots, given the `addresses` the
    /// values hold.
    fn visit(
        &mut self,
        function: &Function,
        addresses: &SecondaryMap<Value, Option<Address>>,
        inst: Inst,
    ) {
        let role = role_of(function, addresses, inst);
        let address_operand = match role {
            Role::Accesses(slot, access, operand) => {
                self.accesses[slot].push(access);
                Some(operand)
            }
            Role::Offsets(base, slot) => {
                self.computed[slot].push(inst);
                Some(base)
            }
            Role::Other => None,
        };
        if let Some(Address { slot, .. }) = stack_addr_of(function, inst) {
            self.computed[slot].push(inst);
        }
        let mut operand_seen = false;
        for value in function.dfg.inst_values(inst) {
            let Some(address) = addresses[value] else {
                continue;
            };
            if address_operand == Some(value) && !operand_seen {
                operand_seen = true;
            } else {
                self.escaped[address.slot] = true;
            }
        }
        self.record_stack_map_escapes(function, inst);
    }

    /// Mark every slot a stack map of `inst` names as escaping.
    fn record_stack_map_escapes(&mut self, function: &Function, inst: Inst) {
        for entry in function
            .dfg
            .user_stack_map_entries(inst)
            .unwrap_or_default()
        {
            self.escaped[entry.slot] = true;
        }
    }

    /// The cells of every slot of `function` that kept its address and
    /// whose accesses tile it.
    fn into_cells(mut self, function: &Function) -> Cells {
        let mut cells = Cells::default();
        for (slot, data) in function.sized_stack_slots.iter() {
            if data.kind != StackSlotKind::ExplicitSlot || data.size == 0 || self.escaped[slot] {
                continue;
            }
            let accesses = std::mem::take(&mut self.accesses[slot]);
            let Some(slot_cells) = tile(slot, i64::from(data.size), accesses) else {
                continue;
            };
            for (cell, members) in slot_cells {
                let index = cells.cells.len();
                for member in members {
                    cells.accesses[member.inst] = Some(Access {
                        cell: index,
                        direction: member.direction,
                    });
                }
                cells.cells.push(cell);
            }
            cells
                .slots
                .push((slot, std::mem::take(&mut self.computed[slot])));
        }
        cells
    }
}

/// The address `inst` computes when it is a `stack_addr` of an explicit
/// slot.
fn stack_addr_of(function: &Function, inst: Inst) -> Option<Address> {
    match function.dfg.insts[inst] {
        InstructionData::StackAddr {
            opcode: Opcode::StackAddr,
            stack_slot,
            offset,
        } if is_explicit(function, stack_slot) => Some(Address {
            slot: stack_slot,
            offset: i64::from(offset),
        }),
        _ => None,
    }
}

/// What `inst` does with an address in a slot, given the `addresses` the
/// values hold.
fn role_of(
    function: &Function,
    addresses: &SecondaryMap<Value, Option<Address>>,
    inst: Inst,
) -> Role {
    access_through_address(function, addresses, inst)
        .unwrap_or_else(|| offset_role(function, addresses, inst))
}

/// The role of `inst` when it is a plain load or store through an address
/// in a slot, given the `addresses` the values hold.
fn access_through_address(
    function: &Function,
    addresses: &SecondaryMap<Value, Option<Address>>,
    inst: Inst,
) -> Option<Role> {
    let dfg = &function.dfg;
    match dfg.insts[inst] {
        InstructionData::Load {
            opcode: Opcode::Load,
            arg,
            flags,
            offset,
        } => {
            let at = through(addresses[arg], dfg.mem_flags[flags], offset.into())?;
            let ty = dfg.value_type(result_of(function, inst)?);
            Some(accessing(inst, at, ty, Direction::Read, arg))
        }
        InstructionData::Store {
            opcode: Opcode::Store,
            args: [data, arg],
            flags,
            offset,
        } if data != arg => {
            let at = through(addresses[arg], dfg.mem_flags[flags], offset.into())?;
            let ty = dfg.value_type(data);
            Some(accessing(inst, at, ty, Direction::Write, arg))
        }
        _ => None,
    }
}

/// The role of `inst` when it adds a constant to an address, given the
/// `addresses` the values hold.
fn offset_role(
    function: &Function,
    addresses: &SecondaryMap<Value, Option<Address>>,
    inst: Inst,
) -> Role {
    let computed = result_of(function, inst).and_then(|result| addresses[result]);
    let base = match function.dfg.insts[inst] {
        InstructionData::Binary {
            opcode: Opcode::Iadd,
            args: [left, _],
        } if addresses[left].is_some() => left,
        InstructionData::Binary {
            opcode: Opcode::Iadd,
            args: [_, right],
        } => right,
        _ => return Role::Other,
    };
    computed.map_or(Role::Other, |address| Role::Offsets(base, address.slot))
}

/// The one result of `inst`.
fn result_of(function: &Function, inst: Inst) -> Option<Value> {
    function.dfg.inst_results(inst).first().copied()
}

/// The byte a plain load or store through an operand holding `address`,
/// with `flags` and `offset`, reaches, when it reaches a slot in native
/// byte order.
fn through(address: Option<Address>, flags: MemFlagsData, offset: i32) -> Option<Address> {
    if flags.explicit_endianness().is_some() {
        return None;
    }
    address?.moved(i64::from(offset))
}

/// The role of `inst` accessing `ty` at `at` in `direction` through the
/// address `operand`.
fn accessing(inst: Inst, at: Address, ty: Type, direction: Direction, operand: Value) -> Role {
    let access = SlotAccess {
        inst,
        offset: at.offset,
        ty,
        direction,
    };
    Role::Accesses(at.slot, access, operand)
}

/// The cells `accesses` of a slot of `size` bytes fall into, each with its
/// accesses sorted by instruction number (the order instructions were
/// created in, not their layout order), when every access lies within the
/// slot, overlaps only accesses of exactly its bytes, and has a type its
/// cell can hold and convert it to.
fn tile(
    slot: StackSlot,
    size: i64,
    mut accesses: Vec<SlotAccess>,
) -> Option<Vec<(Cell, Vec<SlotAccess>)>> {
    accesses.sort_by_key(|access| (access.bytes(), access.inst));
    let mut cells: Vec<(Cell, Vec<SlotAccess>)> = Vec::new();
    let mut end = 0i64;
    let mut previous: Option<(i64, i64)> = None;
    for access in accesses {
        let (offset, length) = access.bytes();
        let access_end = offset.checked_add(length)?;
        if offset < 0 || access_end > size {
            return None;
        }
        if previous == Some((offset, length)) {
            cells.last_mut()?.1.push(access);
            continue;
        }
        if offset < end {
            return None;
        }
        end = access_end;
        previous = Some((offset, length));
        let cell = Cell {
            slot,
            offset: u32::try_from(offset).ok()?,
            ty: access.ty,
        };
        cells.push((cell, vec![access]));
    }
    for (cell, members) in &mut cells {
        cell.ty = cell_type(members)?;
    }
    Some(cells)
}

/// The type a cell accessed by `members`, sorted by instruction number,
/// holds: the first float type among them, or else the first type, when
/// every type is one a cell can hold and can be converted to it.
fn cell_type(members: &[SlotAccess]) -> Option<Type> {
    if !members.iter().all(|member| is_holdable(member.ty)) {
        return None;
    }
    let first = members.first()?.ty;
    let ty = (members.iter())
        .map(|member| member.ty)
        .find(|ty| ty.is_float())
        .unwrap_or(first);
    (members.iter())
        .all(|member| is_convertible(member.ty, ty))
        .then_some(ty)
}

/// Whether a cell can hold a value of type `ty`: an integer, an `f32` or an
/// `f64`, or a fixed-width vector of those.
fn is_holdable(ty: Type) -> bool {
    let is_scalar = |ty: Type| {
        matches!(
            ty,
            types::I8
                | types::I16
                | types::I32
                | types::I64
                | types::I128
                | types::F32
                | types::F64
        )
    };
    let is_fixed_vector = ty.is_vector() && !ty.is_dynamic_vector() && ty.bits() != 0;
    is_scalar(ty) || (is_fixed_vector && is_scalar(ty.lane_type()))
}

/// Whether a value of type `from` can stand for one of type `to` by a
/// bitcast: the same type, or two scalars of the same width.
pub(super) fn is_convertible(from: Type, to: Type) -> bool {
    let is_scalar = |ty: Type| ty.lane_count() == 1 && (ty.is_int() || ty.is_float());
    from == to || (is_scalar(from) && is_scalar(to) && from.bits() == to.bits())
}
