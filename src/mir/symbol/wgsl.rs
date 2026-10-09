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
//! A top-level function is spelled by its declaring module and its name, and
//! a method by its owner and its name, each followed by `__` and each argument
//! token:
//!
//! ```text
//! function  = program | escaped | imported
//! program   = name                  (name holds no `__` and begins with no `m__`)
//! escaped   = "m__0_" name          (name holds `__` or begins with `m__`)
//! imported  = "m__" { length ident }+ "_" imported_name
//! imported_name = name              (name holds no `__`)
//!               | length name       (name holds `__`)
//! method    = "m__t" length owner length name
//!           | "m__q" length "_" token length name
//! length    = decimal length of what follows, no leading zero
//! ```
//!
//! A type a module keeps private is identified as `local.k.Helper`, which no
//! identifier can hold, so wherever its identity is spelled here — a method's
//! owner, a vtable, a thunk — it is spelled as the token a type argument of it
//! takes, `3q_local_k_Helper`. That token begins with a digit, so a method of
//! such an owner takes a mark of its own, and its owner's length is ended by
//! `_` rather than by the owner's first character: the `v` of that `Helper`
//! is `m__q17_3q_local_k_Helper1v`.
//!
//! The program's own `helper` is spelled `helper`, its `m__x` is `m__0_m__x`,
//! its `__h` is `m__0___h`, and the `helper` of `system.math` is
//! `m__6system4math_helper`. The method `norm` of `P` is `m__t1P4norm`. WGSL
//! reserves identifiers beginning `__`; no spelling begins that way, since a
//! `program` name that would is escaped and every other spelling begins `m__`.
//!
//! This is injective up to argument tokens. Only `program` spellings lack the
//! `m__` prefix, and a `program` name holds no `__`, so its first `__` begins
//! its argument tokens. After the prefix, `escaped` continues with `0`,
//! `method` with `t`, or with `q` for an owner a module keeps private, and
//! `imported` with the first digit of a non-empty identifier's length, never
//! `0`. An identifier never begins with a digit, so each length ends where
//! what it counts begins, and the one length counting a token, which does
//! begin with a digit, ends at the `_` after it: an `imported` path ends at
//! the first `_` found where a length would begin, the name after it is
//! length-prefixed exactly when it holds `__`, and a method's owner and name
//! are each exactly as long as their lengths say. Argument tokens are joined
//! with `__`, which a type's spelling may itself hold, and other kinds of
//! symbol share this identifier space; the GPU name table is the backstop for
//! both.

use std::fmt;

use super::token::identifier_spelling;
use super::{
    ClosureKind, GpuKernelKind, KernelDatum, StringLiteralPart, Symbol, SymbolKind, ThunkKind,
    ThunkSubject, Token, ENTRY_NAME,
};
use crate::ast::type_identity;
use crate::type_checker::ModuleId;

/// The separator between a name and each of its argument tokens.
const ARGUMENT_SEPARATOR: &str = "__";

/// Begins the spelling of a function an imported module declares, and of
/// every other spelling that is not a program function's own name.
const MODULE_PREFIX: &str = "m__";

/// Follows [`MODULE_PREFIX`] in the spelling of a method; a module path
/// identifier's length never begins with a letter.
const METHOD_MARK: &str = "t";

/// Follows [`MODULE_PREFIX`] in the spelling of a method of a type a module
/// keeps private.
const QUALIFIED_METHOD_MARK: &str = "q";

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
            write_method_owner(f, owner)?;
            write!(f, "{}{method}", method.len())?;
            write_arguments(f, owner_args)?;
            write_arguments(f, method_args)
        }
        SymbolKind::Vtable { class, args } => {
            write!(f, "__vtable_{}", identifier_spelling(class))?;
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

/// The start of a method's spelling: its mark, then its owner, after the
/// owner's length.
fn write_method_owner(f: &mut fmt::Formatter<'_>, owner: &str) -> fmt::Result {
    if type_identity::is_qualified(owner) {
        let token = identifier_spelling(owner);
        write!(
            f,
            "{MODULE_PREFIX}{QUALIFIED_METHOD_MARK}{}_{token}",
            token.len()
        )
    } else {
        write!(f, "{MODULE_PREFIX}{METHOD_MARK}{}{owner}", owner.len())
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
            f.write_str("_")?;
            if name.contains(ARGUMENT_SEPARATOR) {
                write!(f, "{}", name.len())?;
            }
            f.write_str(name)
        }
    }
}

/// Whether a program function named `name` is spelled escaped: its name
/// would otherwise read as another spelling that begins with the module
/// prefix, begin with the prefix WGSL reserves, or read as a name followed by
/// argument tokens.
fn is_escaped_program_name(name: &str) -> bool {
    name.starts_with(MODULE_PREFIX)
        || name.starts_with(RESERVED_PREFIX)
        || name.contains(ARGUMENT_SEPARATOR)
}

fn thunk_prefix(kind: ThunkKind) -> &'static str {
    match kind {
        ThunkKind::Drop => "__drop_",
        ThunkKind::Decref => "__decref_",
        ThunkKind::Clone => "__clone_",
        ThunkKind::Compare => "__compare_",
        ThunkKind::Equals => "__equals_",
        ThunkKind::Hash => "__hash_",
    }
}

fn write_thunk_subject(f: &mut fmt::Formatter<'_>, subject: &ThunkSubject) -> fmt::Result {
    match subject {
        ThunkSubject::Named { name, args } => {
            f.write_str(&identifier_spelling(name))?;
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
