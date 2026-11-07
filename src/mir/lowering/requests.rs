// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What lowering a body asks the pipeline to lower or settle next: the
//! generic and residency-specialized functions it calls, the generic class
//! instances it names, and the workgroup size it launches each kernel with.
//!
//! Requests travel beside the body they were recorded for, never inside it,
//! so no later pass can read or rewrite them as if they were part of the IR.

use crate::ast::types::Type;
use crate::mir::body::{Body, DeviceHandleId};
use crate::mir::lambda::LambdaInfo;
use crate::mir::symbol::Symbol;
use crate::type_checker::DeclaredFunction;

/// The requests one lowered body makes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoweringRequests {
    /// Every generic function instantiation the body calls, in call order.
    /// Recorded when the call is lowered, with the body's own instantiation
    /// substitution already applied, so the pipeline can lower each callee
    /// without recovering its type arguments from the mangled symbol.
    pub generic_function_calls: Vec<GenericFunctionCall>,
    /// Every generic method instantiation the body calls, in call order,
    /// recorded as [`Self::generic_function_calls`] is: the method's own type
    /// arguments with the body's substitution applied, beside the owner's.
    pub generic_method_calls: Vec<GenericMethodCall>,
    /// Every call the body retargets to a residency-specialized body, in call
    /// order. Recorded when the call is lowered, so the pipeline lowers each
    /// specialization from the function it specializes without recovering that
    /// function from the specialized symbol.
    pub residency_function_calls: Vec<ResidencyFunctionCall>,
    /// Every generic class instantiation the body names only through its own
    /// instantiation substitution, in the order it was met.
    ///
    /// The type checker records the instantiations a program writes down, once,
    /// against the parameters of the body they appear in: `Box<T>` inside
    /// `via_box<T>` is `Box` at a placeholder. Which concrete `Box` that is exists
    /// only once `via_box` is lowered for a type, so the lowering records it and
    /// the pipeline adds it to the registry that decides which per-instantiation
    /// method bodies and drop functions get emitted.
    pub generic_class_instantiations: Vec<GenericClassInstantiation>,
    /// GPU kernel workgroup sizes recorded at the body's launches of kernels.
    /// Maps each kernel to [block_x, block_y, block_z].
    /// Collected when a `kernel(args).launch(grid, block)` call is lowered in a caller.
    /// Applied as metadata to the GPU kernel body's `BackendMetadata::Gpu.workgroup_size`
    /// by `mir::gpu::stamp_kernel_workgroups` once every body is lowered.
    pub kernel_workgroups: Vec<(Symbol, [u32; 3])>,
}

/// A body the pipeline has lowered, under the symbol it is emitted as, with
/// the requests it makes.
#[derive(Debug)]
pub struct LoweredBody {
    pub symbol: Symbol,
    pub body: Body,
    pub requests: LoweringRequests,
}

/// A body as lowering hands it over: the body, the requests it makes, and
/// the closures it builds, each lowered to a body of its own.
#[derive(Debug)]
pub struct Lowered {
    pub body: Body,
    pub requests: LoweringRequests,
    pub lambdas: Vec<LambdaInfo>,
}

/// One generic class at concrete type arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericClassInstantiation {
    /// The generic class's declared name.
    pub class: String,
    /// The class's type arguments, in declaration order.
    pub type_args: Vec<Type>,
}

/// One call to a generic function at concrete type arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericFunctionCall {
    /// The instantiation the call targets, e.g. `smaller` at `String`.
    pub symbol: Symbol,
    /// The generic function the instantiation is lowered from.
    pub function: DeclaredFunction,
    /// Each of the callee's generic parameters, paired with the type it is
    /// instantiated at.
    pub type_args: Vec<(String, Type)>,
}

/// One call to a method declaring type parameters of its own (`fn m<T>`), at
/// concrete type arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericMethodCall {
    /// The instantiation the call targets, e.g. `Box.same` at `String`.
    pub symbol: Symbol,
    /// The class or enum the copy is compiled under, which `self` is typed
    /// as: the one declaring the method, the ancestor a generic class
    /// inherits it from, or the class a trait default or an abstract
    /// ancestor's body is copied into.
    pub owner: String,
    /// The owner's type arguments, in declaration order; empty for an owner
    /// that declares none.
    pub owner_args: Vec<Type>,
    /// The method's name.
    pub method: String,
    /// Each of the method's own type parameters, paired with the type it is
    /// instantiated at.
    pub type_args: Vec<(String, Type)>,
}

/// One call to a function specialized for the gpu-resident buffers it passes.
#[derive(Debug, Clone, PartialEq)]
pub struct ResidencyFunctionCall {
    /// The residency-specialized symbol the call targets.
    pub symbol: Symbol,
    /// The function the specialization is lowered from.
    pub function: DeclaredFunction,
    /// Each generic parameter of the function paired with the type the call
    /// instantiates it at; empty for a function that is not generic.
    pub type_args: Vec<(String, Type)>,
    /// The device handle each positional argument carries, `None` for an
    /// argument that is not a gpu-resident buffer.
    pub arg_handles: Vec<Option<DeviceHandleId>>,
}
