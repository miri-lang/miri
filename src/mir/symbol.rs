// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The names compiled bodies and data are emitted under.
//!
//! A [`Symbol`] records *what* a name stands for — a function at some type
//! arguments, a method of some owner, a closure lowered out of a body, a GPU
//! kernel — and [`Symbol::link_name`] is the one place that spells it. Two
//! symbols are equal when they stand for the same thing, which is a finer
//! question than whether they spell the same link name: a class `A_b` with a
//! method `c` and a class `A` with a method `b_c` are different symbols that
//! today spell one name.
//!
//! Type arguments are held as the tokens
//! [`type_kind_to_mangle_str`] gives them, so a symbol is hashable and
//! comparable without comparing [`Type`]s, and two different types never
//! compare equal.

use std::borrow::Cow;
use std::fmt;

use crate::ast::types::Type;
use crate::mir::body::DeviceHandleId;
use crate::mir::lowering::method_dispatch::type_kind_to_mangle_str;

/// One type argument's token, as [`type_kind_to_mangle_str`] spells it.
type Token = Cow<'static, str>;

/// The separator between a name and each of its argument tokens.
const ARGUMENT_SEPARATOR: &str = "__";

/// The name the program's entry point is linked under.
const ENTRY_NAME: &str = "main";

/// What a lowered body a closure value points at was written as.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClosureKind {
    /// A lambda expression.
    Lambda,
    /// The thunk a reference to a named function is called through; carries
    /// the link name of the function it forwards to.
    FunctionReference(String),
    /// A function declared inside another body; carries its declared name.
    NestedFunction(String),
}

/// The construct a GPU kernel is compiled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuKernelKind {
    /// The body of a `forall` loop.
    Forall,
    /// The fold of a gpu-resident array's `reduce`.
    Reduce,
}

/// A compiled body or datum, identified by what it stands for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    kind: SymbolKind,
    residency: Vec<(usize, DeviceHandleId)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SymbolKind {
    Function {
        name: String,
        args: Vec<Token>,
    },
    Method {
        owner: String,
        owner_args: Vec<Token>,
        method: String,
        method_args: Vec<Token>,
    },
    Vtable {
        class: String,
        args: Vec<Token>,
    },
    Closure {
        kind: ClosureKind,
        id: usize,
        context_args: Vec<Token>,
    },
    GpuKernel {
        kind: GpuKernelKind,
        index: usize,
    },
    Runtime(String),
    Entry,
}

impl Symbol {
    /// The function `name`, instantiated at `args` when it is generic.
    pub fn function<'t>(name: &str, args: impl IntoIterator<Item = &'t Type>) -> Self {
        Self::of(SymbolKind::Function {
            name: name.to_string(),
            args: tokens(args),
        })
    }

    /// The method `method` of `owner`, with the owner's type arguments and the
    /// method's own.
    pub fn method<'o, 'm>(
        owner: &str,
        owner_args: impl IntoIterator<Item = &'o Type>,
        method: &str,
        method_args: impl IntoIterator<Item = &'m Type>,
    ) -> Self {
        Self::of(SymbolKind::Method {
            owner: owner.to_string(),
            owner_args: tokens(owner_args),
            method: method.to_string(),
            method_args: tokens(method_args),
        })
    }

    /// The virtual-dispatch table of `class` at the instantiation `args`.
    pub fn vtable<'t>(class: &str, args: impl IntoIterator<Item = &'t Type>) -> Self {
        Self::of(SymbolKind::Vtable {
            class: class.to_string(),
            args: tokens(args),
        })
    }

    /// A body lowered out of another one, identified by the AST node `id` it
    /// was written at and the types the enclosing lowering was at: its
    /// receiver first, then its substitution in a stable order.
    pub fn closure<'t>(
        kind: ClosureKind,
        id: usize,
        context_args: impl IntoIterator<Item = &'t Type>,
    ) -> Self {
        Self::of(SymbolKind::Closure {
            kind,
            id,
            context_args: tokens(context_args),
        })
    }

    /// The `index`-th GPU kernel of its compilation.
    pub fn gpu_kernel(kind: GpuKernelKind, index: usize) -> Self {
        Self::of(SymbolKind::GpuKernel { kind, index })
    }

    /// A function the runtime library exports under the C name `c_name`.
    pub fn runtime(c_name: &str) -> Self {
        Self::of(SymbolKind::Runtime(c_name.to_string()))
    }

    /// The program's entry point.
    pub fn entry() -> Self {
        Self::of(SymbolKind::Entry)
    }

    /// This symbol specialized for the gpu-resident buffers `handles` passes:
    /// each entry is an argument position and the device handle it carries.
    pub fn with_residency(mut self, handles: &[(usize, DeviceHandleId)]) -> Self {
        self.residency.extend_from_slice(handles);
        self
    }

    /// Whether the runtime library, not this compilation, provides the body.
    pub fn is_runtime(&self) -> bool {
        matches!(self.kind, SymbolKind::Runtime(_))
    }

    /// The name the linker knows this symbol by.
    pub fn link_name(&self) -> String {
        self.to_string()
    }

    /// The name a WGSL module declares this symbol under. A kernel's entry
    /// point is spelled exactly as the host names it when it launches it.
    pub fn wgsl_name(&self) -> String {
        self.link_name()
    }

    fn of(kind: SymbolKind) -> Self {
        Self {
            kind,
            residency: Vec::new(),
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_kind(f, &self.kind)?;
        write_residency(f, &self.residency)
    }
}

fn write_kind(f: &mut fmt::Formatter<'_>, kind: &SymbolKind) -> fmt::Result {
    match kind {
        SymbolKind::Function { name, args } => {
            f.write_str(name)?;
            write_arguments(f, args)
        }
        SymbolKind::Method {
            owner,
            owner_args,
            method,
            method_args,
        } => {
            write!(f, "{owner}_{method}")?;
            write_arguments(f, owner_args)?;
            write_arguments(f, method_args)
        }
        SymbolKind::Vtable { class, args } => {
            write!(f, "__vtable_{class}")?;
            write_arguments(f, args)
        }
        SymbolKind::Closure {
            kind,
            id,
            context_args,
        } => {
            write_closure_base(f, kind, *id)?;
            write_arguments(f, context_args)
        }
        SymbolKind::GpuKernel { kind, index } => write_gpu_kernel(f, *kind, *index),
        SymbolKind::Runtime(c_name) => f.write_str(c_name),
        SymbolKind::Entry => f.write_str(ENTRY_NAME),
    }
}

fn write_closure_base(f: &mut fmt::Formatter<'_>, kind: &ClosureKind, id: usize) -> fmt::Result {
    match kind {
        ClosureKind::Lambda => write!(f, "__lambda_{id}"),
        ClosureKind::FunctionReference(target) => write!(f, "__fnref_{target}_{id}"),
        ClosureKind::NestedFunction(name) => write!(f, "__nested_{name}_{id}"),
    }
}

fn write_gpu_kernel(f: &mut fmt::Formatter<'_>, kind: GpuKernelKind, index: usize) -> fmt::Result {
    match kind {
        GpuKernelKind::Forall => write!(f, "miri_gpu_forall_{index}"),
        GpuKernelKind::Reduce => write!(f, "miri_gpu_reduce_{index}"),
    }
}

fn write_arguments(f: &mut fmt::Formatter<'_>, args: &[Token]) -> fmt::Result {
    args.iter().try_for_each(|token| {
        f.write_str(ARGUMENT_SEPARATOR)?;
        f.write_str(token)
    })
}

/// Each gpu-resident argument contributes its position and device handle, so
/// distinct buffers specialize to distinct bodies and one buffer reused across
/// calls maps to one. The `__gpu` segment can never appear in a user
/// identifier, so a residency-specialized name cannot collide with a user
/// function or a generic instantiation, and the specialized function's own
/// name is recoverable as the text before the first `__`.
fn write_residency(
    f: &mut fmt::Formatter<'_>,
    residency: &[(usize, DeviceHandleId)],
) -> fmt::Result {
    if residency.is_empty() {
        return Ok(());
    }
    f.write_str("__gpu")?;
    residency
        .iter()
        .try_for_each(|(position, handle)| write!(f, "_p{position}h{}", handle.0))
}

fn tokens<'t>(types: impl IntoIterator<Item = &'t Type>) -> Vec<Token> {
    types
        .into_iter()
        .map(|ty| type_kind_to_mangle_str(&ty.kind))
        .collect()
}
