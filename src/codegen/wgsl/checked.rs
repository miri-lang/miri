// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Overflow-checked integer arithmetic in WGSL.
//!
//! WGSL's integer `+`, `-`, `*` and unary `-` wrap, and its `MIN / -1` and
//! `MIN % -1` quietly yield `MIN` and `0`. A kernel's integer arithmetic is
//! instead spelled as a call to a module-scope helper that computes the plain
//! result and, when that result does not fit the integer's width, sets the
//! overflow bit of the module's sticky fault word, [`FAULT_WORD`]. The host
//! reads the word whenever device data reaches it and stops the program with
//! an integer-overflow error when the bit is set.
//!
//! Every integer computes in a 32-bit lane, so `int`, `i32`, `i64` and their
//! unsigned partners are judged at 32 bits, and `i8`, `i16`, `u8` and `u16`
//! at their own widths: their 32-bit result is exact, so it overflowed exactly
//! when it differs from its low bits re-extended.

use crate::ast::gpu_wire::{device_scalar, sub_lane_bits, DeviceScalar};
use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::mir::{BinOp, Operand};
use std::collections::BTreeSet;

/// The module-scope variable a checked helper records an overflow in. The
/// host recognises a kernel that may fault by this name and binds the word at
/// group 1, binding 0.
pub(super) const FAULT_WORD: &str = "miri_fault";

/// The declaration of [`FAULT_WORD`], emitted only by a module that calls a
/// checked helper.
const FAULT_DECLARATION: &str =
    "@group(1) @binding(0) var<storage, read_write> miri_fault: atomic<u32>;";

/// The statement a helper runs when its result does not fit: bit 0 of the
/// fault word means integer overflow.
const RAISE_OVERFLOW: &str = "atomicOr(&miri_fault, 1u);";

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

    /// Whether this operation with `divisor` on its right always fits: a
    /// quotient or remainder by a constant other than `-1`.
    pub(super) fn always_fits_with_divisor(self, divisor: &Operand) -> bool {
        let is_quotient = match self {
            Self::Div | Self::Rem => true,
            Self::Add | Self::Sub | Self::Mul | Self::Neg | Self::Abs => false,
        };
        is_quotient
            && matches!(
                divisor,
                Operand::Constant(constant)
                    if matches!(&constant.literal, Literal::Integer(value)
                        if value.as_i128().is_some_and(|value| value != -1))
            )
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

/// One checked operation at one width: a WGSL helper function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CheckedHelper {
    arith: CheckedArith,
    lane: IntLane,
}

impl CheckedHelper {
    /// The helper for `arith` at `lane`, or `None` when the operation cannot
    /// overflow there: an unsigned quotient, remainder or magnitude always fits.
    pub(super) fn new(arith: CheckedArith, lane: IntLane) -> Option<Self> {
        let always_fits = matches!(
            arith,
            CheckedArith::Div | CheckedArith::Rem | CheckedArith::Abs
        ) && !lane.is_signed();
        (!always_fits).then_some(Self { arith, lane })
    }

    /// The helper's WGSL name. It begins with `_`, which no source name keeps,
    /// and holds `__`, which no function symbol's spelling holds unprefixed.
    pub(super) fn name(self) -> String {
        format!("_checked_{}__{}", self.arith.name(), self.lane.name())
    }

    /// The helper's WGSL definition.
    fn source(self) -> String {
        let ty = self.lane.wgsl_type();
        let params = match self.arith {
            CheckedArith::Neg | CheckedArith::Abs => format!("a: {ty}"),
            CheckedArith::Add
            | CheckedArith::Sub
            | CheckedArith::Mul
            | CheckedArith::Div
            | CheckedArith::Rem => format!("a: {ty}, b: {ty}"),
        };
        format!(
            "fn {}({params}) -> {ty} {{\n    let r = {};\n    if ({}) {{\n        {RAISE_OVERFLOW}\n    }}\n    return {};\n}}\n",
            self.name(),
            self.result(),
            self.overflow_condition(),
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

    /// The test that the true result does not fit the lane.
    fn overflow_condition(self) -> String {
        let min = self.lane.min_value();
        match (self.arith, self.lane.narrow_bits(), self.lane.is_signed()) {
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
            // `new` never builds an unsigned quotient, remainder or magnitude
            // helper.
            (CheckedArith::Div | CheckedArith::Rem | CheckedArith::Abs, None, false) => {
                "false".to_string()
            }
        }
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
