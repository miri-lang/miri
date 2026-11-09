// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checked integer arithmetic and indexing in WGSL.
//!
//! WGSL's integer `+`, `-`, `*` and unary `-` wrap, its `MIN / -1` and
//! `MIN % -1` quietly yield `MIN` and `0`, its `/` and `%` by zero yield the
//! dividend and `0`, and an index outside a buffer reads or writes some other
//! element or none. A kernel's integer arithmetic, and each index nothing
//! proved in range, is instead spelled as a call to a module-scope helper that
//! computes a defined result and, when the host would have stopped the
//! program, sets the bit [`GpuFault`] names for the kind in the module's
//! sticky fault word, [`FAULT_WORD`]. The host reads the word whenever device
//! data reaches it and stops the program with every kind it records.
//!
//! A helper records a fault and returns; it never returns early, so a call to
//! it leaves the caller's control flow uniform and a barrier after it valid.
//!
//! Every integer computes in a 32-bit lane, so `int`, `i32`, `i64` and their
//! unsigned partners are judged at 32 bits, and `i8`, `i16`, `u8` and `u16`
//! at their own widths: their 32-bit result is exact, so it overflowed exactly
//! when it differs from its low bits re-extended.

use crate::abi::gpu_fault::GpuFault;
use crate::ast::gpu_wire::{device_scalar, sub_lane_bits, DeviceScalar};
use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::mir::{BinOp, Operand};
use std::collections::BTreeSet;

/// The module-scope variable a checked helper records a fault in. The host
/// recognises a kernel that may fault by this name and binds the word at
/// group 1, binding 0.
pub(super) const FAULT_WORD: &str = "miri_fault";

/// The declaration of [`FAULT_WORD`], emitted only by a module that calls a
/// checked helper.
const FAULT_DECLARATION: &str =
    "@group(1) @binding(0) var<storage, read_write> miri_fault: atomic<u32>;";

/// The statement a helper runs to record `fault` in the fault word.
fn raise(fault: GpuFault) -> String {
    format!("atomicOr(&{FAULT_WORD}, {}u);", fault.bit())
}

/// An integer operation that can overflow its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CheckedArith {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    /// The magnitude of a signed value, whose minimum has none at its width.
    Abs,
}

impl CheckedArith {
    /// The checked counterpart of a binary operator, or `None` for an
    /// operator whose result always fits (comparisons, bitwise, shifts).
    pub(super) fn of_binop(op: BinOp) -> Option<Self> {
        match op {
            BinOp::Add => Some(Self::Add),
            BinOp::Sub => Some(Self::Sub),
            BinOp::Mul => Some(Self::Mul),
            BinOp::Div => Some(Self::Div),
            BinOp::Rem => Some(Self::Rem),
            BinOp::BitXor
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Eq
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Ne
            | BinOp::Ge
            | BinOp::Gt
            | BinOp::Offset => None,
        }
    }

    /// Whether this operation with `divisor` on its right always fits and
    /// never divides by zero: a quotient or remainder by a constant other
    /// than `0` and `-1`.
    pub(super) fn always_fits_with_divisor(self, divisor: &Operand) -> bool {
        self.zero_divisor_fault().is_some()
            && matches!(
                divisor,
                Operand::Constant(constant)
                    if matches!(&constant.literal, Literal::Integer(value)
                        if value.as_i128().is_some_and(|value| value != -1 && value != 0))
            )
    }

    /// The fault a zero divisor of this operation records, or `None` for an
    /// operation that takes no divisor.
    fn zero_divisor_fault(self) -> Option<GpuFault> {
        match self {
            Self::Div => Some(GpuFault::DivisionByZero),
            Self::Rem => Some(GpuFault::RemainderByZero),
            Self::Add | Self::Sub | Self::Mul | Self::Neg | Self::Abs => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Rem => "rem",
            Self::Neg => "neg",
            Self::Abs => "abs",
        }
    }
}

/// The integer width a device value is judged at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum IntLane {
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
}

impl IntLane {
    /// The lane an integer of `kind` computes in on the device, or `None` for
    /// a kind that is not a device integer.
    pub(super) fn of(kind: &TypeKind) -> Option<Self> {
        match (device_scalar(kind)?, sub_lane_bits(kind)) {
            (DeviceScalar::I32, None) => Some(Self::I32),
            (DeviceScalar::I32, Some(8)) => Some(Self::I8),
            (DeviceScalar::I32, Some(16)) => Some(Self::I16),
            (DeviceScalar::U32, None) => Some(Self::U32),
            (DeviceScalar::U32, Some(8)) => Some(Self::U8),
            (DeviceScalar::U32, Some(16)) => Some(Self::U16),
            (DeviceScalar::I32 | DeviceScalar::U32, Some(_))
            | (DeviceScalar::F16 | DeviceScalar::F32 | DeviceScalar::F64, _) => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
        }
    }

    fn is_signed(self) -> bool {
        match self {
            Self::I8 | Self::I16 | Self::I32 => true,
            Self::U8 | Self::U16 | Self::U32 => false,
        }
    }

    /// The WGSL type of the 32-bit lane.
    fn wgsl_type(self) -> &'static str {
        if self.is_signed() {
            "i32"
        } else {
            "u32"
        }
    }

    /// The width of a lane narrower than 32 bits, or `None` for a full lane.
    fn narrow_bits(self) -> Option<u32> {
        match self {
            Self::I8 | Self::U8 => Some(8),
            Self::I16 | Self::U16 => Some(16),
            Self::I32 | Self::U32 => None,
        }
    }

    /// Zero in the lane's type, as a WGSL expression.
    fn zero(self) -> &'static str {
        if self.is_signed() {
            "0"
        } else {
            "0u"
        }
    }

    /// The lane an index of this lane's type is checked in.
    fn index_lane(self) -> IndexLane {
        if self.is_signed() {
            IndexLane::I32
        } else {
            IndexLane::U32
        }
    }

    /// The smallest value of the lane's type, as a WGSL expression.
    fn min_value(self) -> &'static str {
        match self {
            Self::I8 => "-128",
            Self::I16 => "-32768",
            Self::I32 => "-2147483648",
            Self::U8 | Self::U16 | Self::U32 => "0u",
        }
    }
}

/// The 32-bit lane an index is checked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum IndexLane {
    I32,
    U32,
}

impl IndexLane {
    fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::U32 => "u32",
        }
    }

    /// The test that index `i` lies in `0..len`. A signed index is compared
    /// as unsigned, where a negative one is at least `2^31`: no buffer a
    /// kernel binds holds that many 32-bit lanes, so it lies past any length.
    fn inside_condition(self) -> &'static str {
        match self {
            Self::I32 => "u32(i) < len",
            Self::U32 => "i < len",
        }
    }

    fn zero(self) -> &'static str {
        match self {
            Self::I32 => "0",
            Self::U32 => "0u",
        }
    }
}

/// A WGSL helper function a kernel calls for one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CheckedHelper {
    /// One checked integer operation at one width.
    Arith(ArithHelper),
    /// An index checked against its collection's length.
    Index(IndexLane),
}

impl CheckedHelper {
    /// The helper for `arith` at `lane`, or `None` when the operation can
    /// neither overflow nor fault there: an unsigned magnitude always fits.
    pub(super) fn new(arith: CheckedArith, lane: IntLane) -> Option<Self> {
        let always_fits = arith == CheckedArith::Abs && !lane.is_signed();
        (!always_fits).then_some(Self::Arith(ArithHelper { arith, lane }))
    }

    /// The helper that checks an index computed in `lane`.
    pub(super) fn index(lane: IntLane) -> Self {
        Self::Index(lane.index_lane())
    }

    /// The helper's WGSL name. It begins with `_`, which no source name keeps,
    /// and holds `__`, which no function symbol's spelling holds unprefixed.
    pub(super) fn name(self) -> String {
        match self {
            Self::Arith(helper) => {
                format!("_checked_{}__{}", helper.arith.name(), helper.lane.name())
            }
            Self::Index(lane) => format!("_checked_index__{}", lane.name()),
        }
    }

    /// The helper's WGSL definition.
    fn source(self) -> String {
        match self {
            Self::Arith(helper) => helper.source(&self.name()),
            Self::Index(lane) => index_source(lane, &self.name()),
        }
    }
}

/// The definition of the index check `name` in `lane`: an index below the
/// length is returned as it is; any other records the fault and is replaced
/// by `0`. That stand-in may still lie outside an empty collection, where
/// WGSL's bounds rules keep the access inside the buffer; the program stops
/// at the host's next look either way.
fn index_source(lane: IndexLane, name: &str) -> String {
    let ty = lane.name();
    format!(
        "fn {name}(i: {ty}, len: u32) -> {ty} {{\n    let inside = {};\n    if (!inside) {{\n        {}\n    }}\n    return select({}, i, inside);\n}}\n",
        lane.inside_condition(),
        raise(GpuFault::IndexOutOfBounds),
        lane.zero(),
    )
}

/// One checked integer operation at one width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ArithHelper {
    arith: CheckedArith,
    lane: IntLane,
}

impl ArithHelper {
    /// The helper's WGSL definition, named `name`: the plain result, a guard
    /// recording a zero divisor, and one recording a result that does not
    /// fit. WGSL defines both a quotient by zero and `MIN / -1`, so the
    /// result is computed before either guard.
    fn source(self, name: &str) -> String {
        let ty = self.lane.wgsl_type();
        let params = match self.arith {
            CheckedArith::Neg | CheckedArith::Abs => format!("a: {ty}"),
            CheckedArith::Add
            | CheckedArith::Sub
            | CheckedArith::Mul
            | CheckedArith::Div
            | CheckedArith::Rem => format!("a: {ty}, b: {ty}"),
        };
        let zero_divisor = self
            .arith
            .zero_divisor_fault()
            .map(|fault| guard(&format!("b == {}", self.lane.zero()), fault));
        let overflow = self
            .overflow_condition()
            .map(|condition| guard(&condition, GpuFault::IntegerOverflow));
        format!(
            "fn {name}({params}) -> {ty} {{\n    let r = {};\n{}{}    return {};\n}}\n",
            self.result(),
            zero_divisor.unwrap_or_default(),
            overflow.unwrap_or_default(),
            self.returned(),
        )
    }

    /// The plain, possibly overflowed, result.
    fn result(self) -> &'static str {
        match (self.arith, self.lane.is_signed()) {
            (CheckedArith::Add, _) => "a + b",
            (CheckedArith::Sub, _) => "a - b",
            (CheckedArith::Mul, _) => "a * b",
            (CheckedArith::Div, _) => "a / b",
            (CheckedArith::Rem, _) => "a % b",
            (CheckedArith::Neg, true) => "-a",
            (CheckedArith::Neg, false) => "0u - a",
            (CheckedArith::Abs, _) => "abs(a)",
        }
    }

    /// The test that the true result does not fit the lane, or `None` when
    /// it always fits: an unsigned quotient, remainder or magnitude.
    fn overflow_condition(self) -> Option<String> {
        let min = self.lane.min_value();
        let condition = match (self.arith, self.lane.narrow_bits(), self.lane.is_signed()) {
            (CheckedArith::Div | CheckedArith::Rem | CheckedArith::Abs, _, false) => return None,
            (CheckedArith::Div | CheckedArith::Rem, _, true) => {
                format!("a == {min} && b == -1")
            }
            (_, Some(bits), _) => format!("{} != r", narrow_expr("r", bits)),
            (CheckedArith::Add, None, true) => "((a ^ r) & (b ^ r)) < 0".to_string(),
            (CheckedArith::Sub, None, true) => "((a ^ b) & (a ^ r)) < 0".to_string(),
            (CheckedArith::Mul, None, true) => format!(
                "(a == {min} && b == -1) || (b == {min} && a == -1) || (b != 0 && r / b != a)"
            ),
            (CheckedArith::Neg | CheckedArith::Abs, None, true) => format!("a == {min}"),
            (CheckedArith::Add, None, false) => "r < a".to_string(),
            (CheckedArith::Sub, None, false) => "a < b".to_string(),
            (CheckedArith::Mul, None, false) => "b != 0u && r / b != a".to_string(),
            (CheckedArith::Neg, None, false) => "a != 0u".to_string(),
        };
        Some(condition)
    }

    /// What the helper returns: the result, brought back to its own width
    /// when the lane is narrower than 32 bits.
    fn returned(self) -> String {
        match self.lane.narrow_bits() {
            Some(bits) => narrow_expr("r", bits),
            None => "r".to_string(),
        }
    }
}

/// A helper statement recording `fault` when `condition` holds.
fn guard(condition: &str, fault: GpuFault) -> String {
    format!(
        "    if ({condition}) {{\n        {}\n    }}\n",
        raise(fault)
    )
}

/// `expr`, a 32-bit lane value, brought to its low `bits` bits: sign-extended
/// for a signed lane, zero-extended for an unsigned one. `extractBits`, unlike
/// a shift pair, cannot overflow when naga folds it over a constant.
pub(super) fn narrow_expr(expr: &str, bits: u32) -> String {
    format!("extractBits({expr}, 0u, {bits}u)")
}

/// The fault-word declaration and the definition of every helper in
/// `helpers`, sorted by name; empty when the module calls none. Appended after
/// the kernel code so no line the source map records moves.
pub(super) fn checked_section(helpers: &BTreeSet<CheckedHelper>) -> String {
    if helpers.is_empty() {
        return String::new();
    }
    let mut named: Vec<(String, CheckedHelper)> = helpers
        .iter()
        .map(|helper| (helper.name(), *helper))
        .collect();
    named.sort_by(|(a, _), (b, _)| a.cmp(b));
    let mut section = format!("{FAULT_DECLARATION}\n\n");
    for (_, helper) in named {
        section.push_str(&helper.source());
        section.push('\n');
    }
    section
}
