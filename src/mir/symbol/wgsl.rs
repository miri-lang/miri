// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a symbol is spelled where only identifier characters are admitted: the
//! name a WGSL module declares a kernel entry point or a helper under, and the
//! names of the data the compiler emits beside kernels and string literals.
//!
//! WGSL identifiers admit neither `.` nor `$`, so this spelling joins the
//! parts of a symbol with `_` and `__`. A user identifier may contain those
//! too, so two symbols can share this spelling; the link name, not this one,
//! is what keeps every compiled body apart, and the GPU name table refuses two
//! definitions that share it.
//!
//! A top-level function is spelled by its declaring module and its name,
//! followed by `__` and each argument token:
//!
//! ```text
//! function  = program | escaped | imported
//! program   = name                  (name begins with neither `m__` nor `__`)
//! escaped   = "m__0_" name          (name begins with `m__` or `__`)
//! imported  = "m__" { length ident }+ "_" name
//! length    = decimal length of the ident, no leading zero
//! ```
//!
//! The program's own `helper` is spelled `helper`, its `m__x` is `m__0_m__x`,
//! its `__h` is `m__0___h`, and the `helper` of `system.math` is
//! `m__6system4math_helper`. WGSL reserves identifiers beginning `__`; no
//! function spelling begins that way, since a `program` name that would is
//! escaped and every other spelling begins `m__`.
//!
//! Without argument tokens this is injective. Only `program` spellings lack
//! the `m__` prefix, and on them the spelling is the name itself. After the
//! prefix, `escaped` continues with `0` while `imported` continues with the
//! first digit of a non-empty identifier's length, never `0`. An `imported`
//! spelling decodes one way: an identifier never begins with a digit, so each
//! length ends where its identifier begins, the identifier is exactly that
//! many bytes, and the path ends at the first `_` found where a length would
//! begin. Argument tokens are appended after `__`, which a name may itself
//! contain, and other kinds of symbol share this identifier space; the GPU
//! name table is the backstop for both.

use std::fmt;

use super::{
    ClosureKind, GpuKernelKind, KernelDatum, StringLiteralPart, Symbol, SymbolKind, ThunkKind,
    ThunkSubject, Token, ENTRY_NAME,
};
use crate::type_checker::ModuleId;

/// The separator between a name and each of its argument tokens.
const ARGUMENT_SEPARATOR: &str = "__";

/// Begins the spelling of a function an imported module declares.
const MODULE_PREFIX: &str = "m__";

/// Follows [`MODULE_PREFIX`] in the spelling of a program function whose name
/// begins with that prefix or with [`RESERVED_PREFIX`]; a module path
/// identifier's length never begins with `0`.
const PROGRAM_ESCAPE: &str = "0_";

/// The prefix WGSL reserves: no identifier a module declares may begin with it.
const RESERVED_PREFIX: &str = "__";

/// A [`Symbol`] displayed in its identifier-only spelling.
pub(super) struct Wgsl<'s>(pub(super) &'s Symbol);

impl fmt::Display for Wgsl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_kind(f, &self.0.kind)?;
        write_residency(f, &self.0.residency)
    }
}

/// The identifier-only spelling of `kind`, without its residency.
pub(super) fn write_kind(f: &mut fmt::Formatter<'_>, kind: &SymbolKind) -> fmt::Result {
    match kind {
        SymbolKind::Function { module, name, args } => {
            write_function_name(f, module, name)?;
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

/// The spelling of the function `name` that `module` declares, before its
/// argument tokens.
fn write_function_name(f: &mut fmt::Formatter<'_>, module: &ModuleId, name: &str) -> fmt::Result {
    match module {
        ModuleId::Program if is_escaped_program_name(name) => {
            write!(f, "{MODULE_PREFIX}{PROGRAM_ESCAPE}{name}")
        }
        ModuleId::Program => f.write_str(name),
        ModuleId::Imported(path) => {
            f.write_str(MODULE_PREFIX)?;
            path.iter()
                .try_for_each(|segment| write!(f, "{}{segment}", segment.len()))?;
            write!(f, "_{name}")
        }
    }
}

/// Whether a program function named `name` is spelled escaped: its name
/// would otherwise read as a module function's spelling or begin with the
/// prefix WGSL reserves.
fn is_escaped_program_name(name: &str) -> bool {
    name.starts_with(MODULE_PREFIX) || name.starts_with(RESERVED_PREFIX)
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
        GpuKernelKind::FramePass { pass } => write!(f, "miri_gpu_for_{index}_{pass}"),
    }
}

fn write_arguments(f: &mut fmt::Formatter<'_>, args: &[Token]) -> fmt::Result {
    args.iter().try_for_each(|token| {
        f.write_str(ARGUMENT_SEPARATOR)?;
        f.write_str(token)
    })
}

fn write_residency(
    f: &mut fmt::Formatter<'_>,
    residency: &[(usize, crate::mir::body::DeviceHandleId)],
) -> fmt::Result {
    if residency.is_empty() {
        return Ok(());
    }
    f.write_str("__gpu")?;
    residency
        .iter()
        .try_for_each(|(position, handle)| write!(f, "_p{position}h{}", handle.0))
}
