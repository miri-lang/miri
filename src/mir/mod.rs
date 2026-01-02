// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

pub mod block;
pub mod body;
pub mod lowering;
pub mod operand;
pub mod place;
pub mod rvalue;
pub mod statement;
pub mod terminator;
pub mod visitor;

pub use block::{BasicBlock, BasicBlockData};
pub use body::{Body, LocalDecl};
pub use operand::{Constant, Operand};
pub use place::{Local, Place, PlaceElem};
pub use rvalue::{BinOp, Rvalue, UnOp};
pub use statement::{Statement, StatementKind};
pub use terminator::{Terminator, TerminatorKind};
