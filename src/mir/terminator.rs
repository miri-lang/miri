// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::mir::block::BasicBlock;
use crate::mir::operand::Operand;
use crate::mir::place::Place;
use std::fmt;

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
    SwitchInt {
        discr: Operand,
        targets: Vec<(u128, BasicBlock)>,
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
