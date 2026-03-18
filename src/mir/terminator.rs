// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::mir::block::BasicBlock;
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
            TerminatorKind::Call { target, .. } | TerminatorKind::GpuLaunch { target, .. } => {
                target.iter().copied().collect()
            }
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
            TerminatorKind::Call { target, .. } | TerminatorKind::GpuLaunch { target, .. } => {
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
            } => {
                write!(f, "{} = {}(", destination, func)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
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
                destination,
                target,
            } => {
                write!(
                    f,
                    "{} = launch({}, grid: {}, block: {}) -> ",
                    destination, kernel, grid, block
                )?;
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
        destination: Place,
        target: Option<BasicBlock>,
    },
    /// GPU Kernel Launch.
    GpuLaunch {
        kernel: Operand,
        grid: Operand,
        block: Operand,
        destination: Place,
        target: Option<BasicBlock>,
    },
}
