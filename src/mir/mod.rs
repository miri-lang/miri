// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Mid-level Intermediate Representation (MIR) for Miri.
//!
//! MIR is a control-flow graph representation of functions, designed as an
//! intermediate step between the AST and backend code generation. It provides:
//!
//! - Explicit control flow via basic blocks and terminators
//! - SSA-like representation with explicit local variables
//! - Support for GPU execution models and intrinsics
//!
//! MIR is designed to be lowered to multiple backends including LLVM, Cranelift,
//! CUDA/PTX, Metal, and SPIR-V.

pub mod backend;
pub mod block;
pub mod body;
pub mod lowering;
pub mod operand;
pub mod place;
pub mod rvalue;
pub mod statement;
pub mod terminator;
pub mod visitor;

pub use backend::{
    BackendMetadata, GpuAtomicOp, GpuBinding, GpuBodyMetadata, GpuCapability, GpuKernelArg,
    GpuMemoryAccess, GpuMemoryScope,
};
pub use block::{BasicBlock, BasicBlockData};
pub use body::{Body, ExecutionModel, LocalDecl, StorageClass};
pub use operand::{Constant, Operand};
pub use place::{Local, Place, PlaceElem};
pub use rvalue::{AggregateKind, BinOp, Dimension, GpuIntrinsic, Rvalue, UnOp};
pub use statement::{Statement, StatementKind};
pub use terminator::{Terminator, TerminatorKind};
