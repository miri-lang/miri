// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The names compiled bodies and data are emitted under.
//!
//! A [`Symbol`] records *what* a name stands for — a function at some type
//! arguments, a method of some owner, a closure lowered out of a body, a GPU
//! kernel, a per-type function codegen emits for the runtime to call through,
//! a datum beside a kernel or a string literal — and [`Symbol::link_name`] is
//! the one place that spells it. MIR lowering and codegen both compose names
//! through it.
//!
//! Two symbols are equal when they stand for the same thing, which is a finer
//! question than whether they spell the same link name. The spelling joins
//! user identifiers with `_` and `__`, which a user identifier may itself
//! contain, so different symbols can spell one name: a class `A_b` with a
//! method `c` and a class `A` with a method `b_c`, a generic `pick` at `int`
//! and a function written `pick__int`, the drop function of a type and that of
//! a type whose name continues it. Comparing symbols, never link names, is
//! what keeps those apart.
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

/// A function codegen emits for one type, which a release site or the runtime
/// library calls through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThunkKind {
    /// Runs the type's drop hook, releases its managed fields and frees it.
    Drop,
    /// Releases one reference to a value, dropping it when that was the last.
    Decref,
    /// Copies a value through the type's `clone`.
    Clone,
    /// Orders two elements through the type's `compare`.
    Compare,
    /// Matches two elements through the type's `equals`.
    Equals,
}

/// A datum emitted beside a GPU kernel for the host to launch it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelDatum {
    /// The kernel's WGSL source.
    Wgsl,
    /// The kernel's entry-point name.
    Name,
}

/// One of the two data a string literal is emitted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringLiteralPart {
    /// The literal's bytes.
    Bytes,
    /// The immortal string object pointing at those bytes.
    Object,
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
    TypeThunk {
        kind: ThunkKind,
        subject: ThunkSubject,
    },
    ClosureDestructor(String),
    KernelDatum {
        kernel: String,
        datum: KernelDatum,
    },
    StringLiteral {
        index: usize,
        part: StringLiteralPart,
    },
}

/// The type a [`ThunkKind`] function is emitted for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ThunkSubject {
    /// A declared type, at its type arguments when it is generic.
    Named { name: String, args: Vec<Token> },
    /// A tuple, option or function value, which has no declaration to name and
    /// is identified by the encoding of its structure codegen gives it.
    Structural(String),
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

    /// The `kind` function emitted for the declared type `type_name`,
    /// instantiated at `args` when it is generic.
    pub fn type_thunk<'t>(
        kind: ThunkKind,
        type_name: &str,
        args: impl IntoIterator<Item = &'t Type>,
    ) -> Self {
        Self::of(SymbolKind::TypeThunk {
            kind,
            subject: ThunkSubject::Named {
                name: type_name.to_string(),
                args: tokens(args),
            },
        })
    }

    /// The `kind` function emitted for a structural type — a tuple, an option
    /// or a function value — identified by `encoding`, the spelling codegen
    /// gives its structure.
    pub fn structural_thunk(kind: ThunkKind, encoding: &str) -> Self {
        Self::of(SymbolKind::TypeThunk {
            kind,
            subject: ThunkSubject::Structural(encoding.to_string()),
        })
    }

    /// The destructor releasing the captures of a closure value whose body is
    /// linked as `closure`.
    pub fn closure_destructor(closure: &str) -> Self {
        Self::of(SymbolKind::ClosureDestructor(closure.to_string()))
    }

    /// The `datum` emitted for the GPU kernel whose WGSL entry point is
    /// `kernel`.
    pub fn kernel_datum(kernel: &str, datum: KernelDatum) -> Self {
        Self::of(SymbolKind::KernelDatum {
            kernel: kernel.to_string(),
            datum,
        })
    }

    /// The `part` of the `index`-th distinct string literal of a module.
    pub fn string_literal(index: usize, part: StringLiteralPart) -> Self {
        Self::of(SymbolKind::StringLiteral { index, part })
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
        SymbolKind::TypeThunk { kind, subject } => {
            f.write_str(thunk_prefix(*kind))?;
            write_thunk_subject(f, subject)
        }
        SymbolKind::ClosureDestructor(closure) => write!(f, "__dtor_{closure}"),
        SymbolKind::KernelDatum { kernel, datum } => write_kernel_datum(f, kernel, *datum),
        SymbolKind::StringLiteral { index, part } => write_string_literal(f, *index, *part),
    }
}

fn thunk_prefix(kind: ThunkKind) -> &'static str {
    match kind {
        ThunkKind::Drop => "__drop_",
        ThunkKind::Decref => "__decref_",
        ThunkKind::Clone => "__clone_",
        ThunkKind::Compare => "__compare_",
        ThunkKind::Equals => "__equals_",
    }
}

fn write_thunk_subject(f: &mut fmt::Formatter<'_>, subject: &ThunkSubject) -> fmt::Result {
    match subject {
        ThunkSubject::Named { name, args } => {
            f.write_str(name)?;
            write_arguments(f, args)
        }
        ThunkSubject::Structural(encoding) => f.write_str(encoding),
    }
}

fn write_kernel_datum(f: &mut fmt::Formatter<'_>, kernel: &str, datum: KernelDatum) -> fmt::Result {
    match datum {
        KernelDatum::Wgsl => write!(f, "__miri_kernel_{kernel}_wgsl"),
        KernelDatum::Name => write!(f, "__miri_kernel_{kernel}_name"),
    }
}

fn write_string_literal(
    f: &mut fmt::Formatter<'_>,
    index: usize,
    part: StringLiteralPart,
) -> fmt::Result {
    match part {
        StringLiteralPart::Bytes => write!(f, ".miri_str_{index}_bytes"),
        StringLiteralPart::Object => write!(f, ".miri_str_{index}_struct"),
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
/// calls maps to one.
///
/// The suffix is joined with `__gpu_p…h…`, a spelling a user identifier may
/// also contain: a function named `f__gpu_p0h1` spells the same name as `f`
/// specialized for handle 1 at position 0, and nothing here tells the two
/// apart. Nothing reads the specialized function back out of this name — the
/// lowering records which function each specialized call targets — so the
/// only exposure is that collision.
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
