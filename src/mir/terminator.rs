// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::mir::block::BasicBlock;
use crate::mir::body::DeviceHandleId;
use crate::mir::operand::Operand;
use crate::mir::place::Place;
use std::fmt;

/// A discriminant value used in switch statements.
///
/// This newtype wrapper provides type safety for discriminant values,
/// ensuring they are not accidentally confused with other `u128` values.
/// Discriminants are used in `SwitchInt` terminators to match against
/// enum variants, boolean values, or integer patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discriminant(pub u128);

impl Discriminant {
    /// Creates a new discriminant from a `u128` value.
    pub fn new(value: u128) -> Self {
        Self(value)
    }

    /// Returns the underlying `u128` value.
    pub fn value(self) -> u128 {
        self.0
    }

    /// Creates a discriminant representing `true` (1).
    pub fn bool_true() -> Self {
        Self(1)
    }

    /// Creates a discriminant representing `false` (0).
    pub fn bool_false() -> Self {
        Self(0)
    }
}

impl From<u128> for Discriminant {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl From<i32> for Discriminant {
    fn from(value: i32) -> Self {
        Self(value as u128)
    }
}

impl From<usize> for Discriminant {
    fn from(value: usize) -> Self {
        Self(value as u128)
    }
}

impl fmt::Display for Discriminant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Terminator {
    pub span: Span,
    pub kind: TerminatorKind,
}

impl Terminator {
    pub fn new(kind: TerminatorKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn successors(&self) -> Vec<BasicBlock> {
        match &self.kind {
            TerminatorKind::Goto { target } => vec![*target],
            TerminatorKind::SwitchInt {
                targets, otherwise, ..
            } => {
                let mut succs = Vec::with_capacity(targets.len() + 1);
                for (_, target) in targets {
                    succs.push(*target);
                }
                succs.push(*otherwise);
                succs
            }
            TerminatorKind::Return | TerminatorKind::Unreachable => vec![],
            TerminatorKind::Call { target, .. }
            | TerminatorKind::GpuLaunch { target, .. }
            | TerminatorKind::VirtualCall { target, .. } => target.iter().copied().collect(),
        }
    }

    pub fn replace_successor(&mut self, old: BasicBlock, new: BasicBlock) {
        match &mut self.kind {
            TerminatorKind::Goto { target } => {
                if *target == old {
                    *target = new;
                }
            }
            TerminatorKind::SwitchInt {
                targets, otherwise, ..
            } => {
                for (_, target) in targets {
                    if *target == old {
                        *target = new;
                    }
                }
                if *otherwise == old {
                    *otherwise = new;
                }
            }
            TerminatorKind::Call { target, .. }
            | TerminatorKind::GpuLaunch { target, .. }
            | TerminatorKind::VirtualCall { target, .. } => {
                if let Some(t) = target {
                    if *t == old {
                        *t = new;
                    }
                }
            }
            TerminatorKind::Return | TerminatorKind::Unreachable => {}
        }
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TerminatorKind::Goto { target } => write!(f, "goto {}", target),
            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            } => {
                write!(f, "switchInt({}) -> [", discr)?;
                for (val, target) in targets {
                    write!(f, "{}: {}, ", val, target)?;
                }
                write!(f, "otherwise: {}]", otherwise)
            }
            TerminatorKind::Return => write!(f, "return"),
            TerminatorKind::Unreachable => write!(f, "unreachable"),
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                ..
            } => {
                write!(f, "{} = {}(", destination, func)?;
                if let Some((first, rest)) = args.split_first() {
                    write!(f, "{}", first)?;
                    for arg in rest {
                        write!(f, ", {}", arg)?;
                    }
                }
                write!(f, ") -> ")?;
                if let Some(t) = target {
                    write!(f, "{}", t)
                } else {
                    write!(f, "unwind")
                }
            }
            TerminatorKind::GpuLaunch {
                kernel,
                grid,
                block,
                args,
                arg_handles: _,
                arg_read_only: _,
                arg_int_narrow: _,
                scalar_args: _,
                uniform_bound_x: _,
                uniform_bound_y: _,
                uniform_bound_z: _,
                destination,
                target,
            } => {
                write!(
                    f,
                    "{} = launch({}, grid: {}, block: {}",
                    destination, kernel, grid, block
                )?;
                for arg in args {
                    write!(f, ", {}", arg)?;
                }
                write!(f, ") -> ")?;
                if let Some(t) = target {
                    write!(f, "{}", t)
                } else {
                    write!(f, "unwind")
                }
            }
            TerminatorKind::VirtualCall {
                vtable_slot,
                args,
                out_args: _,
                destination,
                target,
            } => {
                write!(f, "{} = vcall[{}](", destination, vtable_slot)?;
                if let Some((first, rest)) = args.split_first() {
                    write!(f, "{}", first)?;
                    for arg in rest {
                        write!(f, ", {}", arg)?;
                    }
                }
                write!(f, ") -> ")?;
                if let Some(t) = target {
                    write!(f, "{}", t)
                } else {
                    write!(f, "unwind")
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TerminatorKind {
    /// Jump to the target block.
    Goto { target: BasicBlock },
    /// Switch based on an integer value.
    ///
    /// The `discr` operand is evaluated, and control transfers to the first
    /// target whose discriminant matches the value. If no match is found,
    /// control transfers to `otherwise`.
    SwitchInt {
        /// The operand being discriminated on.
        discr: Operand,
        /// List of (discriminant, target) pairs.
        targets: Vec<(Discriminant, BasicBlock)>,
        /// The fallback target if no discriminant matches.
        otherwise: BasicBlock,
    },
    /// Return from the function.
    Return,
    /// Indicates that the program execution should never reach this point.
    Unreachable,
    /// Function call.
    Call {
        func: Operand,
        args: Vec<Operand>,
        /// Which arguments are `out` parameters. `out_args[i]` is true when
        /// the callee's i-th parameter is declared `out`. Empty means none.
        out_args: Vec<bool>,
        destination: Place,
        target: Option<BasicBlock>,
    },
    /// GPU Kernel Launch.
    ///
    /// `args` are the host-side capture operands marshaled into the kernel's
    /// storage buffers (binding 0..N in declaration order). Each capture's
    /// read/write mode is recorded in `arg_read_only`.
    ///
    /// `scalar_args` are host-side scalar values (int, bool, f32) that are
    /// passed as WGSL uniform values. These are read-only in the kernel.
    /// The runtime packs them into a uniform struct buffer and passes it to
    /// the kernel at binding index `num_buffers + num_bound_uniforms`.
    ///
    /// When `uniform_bound_x`, `uniform_bound_y`, or `uniform_bound_z` is `Some`, the kernel
    /// bounds-check loop limit(s) are exposed as a uniform buffer instead of compile-time
    /// constants. The runtime will write the values to the uniform buffer before dispatch.
    /// For 1D loops, only `uniform_bound_x` is used; for 2D loops, both x and y may be used;
    /// for 3D loops, all three may be used.
    GpuLaunch {
        kernel: Operand,
        grid: Operand,
        block: Operand,
        args: Vec<Operand>,
        /// Persistent device-buffer id for each capture in `args`, in the same
        /// order. `Some` marks a `gpu`-resident capture whose device buffer
        /// survives across launches; `None` marks a host-resident capture
        /// uploaded and read back per launch. Empty means every capture is
        /// host-resident.
        arg_handles: Vec<Option<DeviceHandleId>>,
        /// Which args (storage buffers) are read-only.
        /// `arg_read_only[i]` is true when the i-th capture is never written to
        /// in the kernel body. Empty means no captures (impossible), or this
        /// launch is legacy and all captures are assumed read_write.
        arg_read_only: Vec<bool>,
        /// Which args need i64→i32 narrowing on upload and i32→i64 widening on readback.
        /// `arg_int_narrow[i]` is true when the i-th capture is an `Array<int, N>`.
        /// Empty means no captures (impossible), or this launch is legacy and none need narrowing.
        arg_int_narrow: Vec<bool>,
        /// Scalar captures (int, bool, f32) passed as WGSL uniform values.
        /// Empty if no scalar captures. Each entry is materialized to a local
        /// and its type is available from the type checker.
        scalar_args: Vec<Operand>,
        /// When present, an i64 operand containing the loop-bound limit value for the x axis.
        /// For 1D loops, this carries the single bound. For 2D loops, this is the width.
        /// For 3D loops, this is the width.
        /// This is lowered to a uniform buffer in the kernel. When `None`, x bounds
        /// are compile-time constants.
        uniform_bound_x: Option<Box<Operand>>,
        /// When present, an i64 operand containing the loop-bound limit value for the y axis.
        /// For 2D loops only; for 3D loops, this is the height.
        /// `None` for 1D loops or literal bounds.
        uniform_bound_y: Option<Box<Operand>>,
        /// When present, an i64 operand containing the loop-bound limit value for the z axis.
        /// For 3D loops only; `None` for 1D/2D loops or literal bounds.
        uniform_bound_z: Option<Box<Operand>>,
        destination: Place,
        target: Option<BasicBlock>,
    },
    /// Virtual method call dispatched through the receiver's vtable.
    ///
    /// `args[0]` is the receiver (self); its vtable pointer is loaded from
    /// `receiver[0]`. The function pointer is loaded from `vtable[vtable_slot * ptr_size]`
    /// and called with all `args`.
    VirtualCall {
        /// Slot index into the receiver's vtable.
        vtable_slot: usize,
        /// Arguments: args[0] is the receiver, rest are method arguments.
        args: Vec<Operand>,
        /// Which arguments are `out` parameters. `out_args[i]` is true when
        /// the callee's i-th parameter is declared `out`. Empty means none.
        out_args: Vec<bool>,
        destination: Place,
        target: Option<BasicBlock>,
    },
}
