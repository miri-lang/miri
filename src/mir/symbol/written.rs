// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How the source writes the definition a symbol names, for a diagnostic
//! that names a definition to the reader rather than by its link name.

use std::fmt;

use super::{
    ClosureKind, GpuKernelKind, KernelDatum, StringLiteralPart, Symbol, SymbolKind, ThunkKind,
    ThunkSubject, Token,
};

/// A [`Symbol`] displayed as the definition it names: `pick<int>`,
/// `local.shapes.area`, `Point.norm`, the drop function of `Box<String>`. A
/// function an imported module declares is shown under that module's path.
pub struct Written<'s>(&'s Symbol);

impl Symbol {
    /// This symbol as the source writes the definition it names.
    pub fn written(&self) -> Written<'_> {
        Written(self)
    }
}

impl fmt::Display for Written<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_written_kind(f, &self.0.kind)?;
        if self.0.residency.is_empty() {
            return Ok(());
        }
        f.write_str(" specialized for its gpu-resident arguments")
    }
}

fn write_written_kind(f: &mut fmt::Formatter<'_>, kind: &SymbolKind) -> fmt::Result {
    match kind {
        SymbolKind::Function { module, name, args } => {
            f.write_str("`")?;
            module
                .path()
                .iter()
                .try_for_each(|segment| write!(f, "{segment}."))?;
            f.write_str(name)?;
            write_type_arguments(f, args)?;
            f.write_str("`")
        }
        SymbolKind::Method {
            owner,
            owner_args,
            method,
            method_args,
        } => {
            write!(f, "`{owner}")?;
            write_type_arguments(f, owner_args)?;
            write!(f, ".{method}")?;
            write_type_arguments(f, method_args)?;
            f.write_str("`")
        }
        SymbolKind::Vtable { class, args } => {
            write!(f, "the vtable of `{class}")?;
            write_type_arguments(f, args)?;
            f.write_str("`")
        }
        SymbolKind::Closure { kind, .. } => write_written_closure(f, kind),
        SymbolKind::GpuKernel { kind, .. } => write_written_kernel(f, *kind),
        SymbolKind::Runtime(c_name) => write!(f, "the runtime function `{c_name}`"),
        SymbolKind::Entry => f.write_str("the entry point `main`"),
        SymbolKind::TypeThunk { kind, subject } => {
            write!(f, "the {} function of ", thunk_role(*kind))?;
            write_written_subject(f, subject)
        }
        SymbolKind::ClosureDestructor(closure) => {
            write!(f, "the capture destructor of `{closure}`")
        }
        SymbolKind::KernelDatum { kernel, datum } => match datum {
            KernelDatum::Wgsl => write!(f, "the WGSL source of kernel `{kernel}`"),
            KernelDatum::Name => write!(f, "the entry-point name of kernel `{kernel}`"),
        },
        SymbolKind::StringLiteral { index, part } => match part {
            StringLiteralPart::Bytes => write!(f, "the bytes of string literal {index}"),
            StringLiteralPart::Object => write!(f, "the object of string literal {index}"),
        },
    }
}

fn write_written_closure(f: &mut fmt::Formatter<'_>, kind: &ClosureKind) -> fmt::Result {
    match kind {
        ClosureKind::Lambda => f.write_str("a lambda expression"),
        ClosureKind::FunctionReference(target) => write!(f, "the reference to `{target}`"),
        ClosureKind::NestedFunction(name) => write!(f, "the nested function `{name}`"),
    }
}

fn write_written_kernel(f: &mut fmt::Formatter<'_>, kind: GpuKernelKind) -> fmt::Result {
    match kind {
        GpuKernelKind::Forall => f.write_str("a `forall` kernel"),
        GpuKernelKind::Reduce => f.write_str("a `reduce` kernel"),
        GpuKernelKind::FramePass { pass } => write!(f, "pass {pass} of a frame kernel"),
    }
}

fn thunk_role(kind: ThunkKind) -> &'static str {
    match kind {
        ThunkKind::Drop => "drop",
        ThunkKind::Decref => "release",
        ThunkKind::Clone => "clone",
        ThunkKind::Compare => "compare",
        ThunkKind::Equals => "equals",
    }
}

fn write_written_subject(f: &mut fmt::Formatter<'_>, subject: &ThunkSubject) -> fmt::Result {
    match subject {
        ThunkSubject::Named { name, args } => {
            write!(f, "`{name}")?;
            write_type_arguments(f, args)?;
            f.write_str("`")
        }
        ThunkSubject::Structural(encoding) => write!(f, "the structural type `{encoding}`"),
    }
}

/// Each argument between angle brackets; one with no name shows as `…`.
fn write_type_arguments(f: &mut fmt::Formatter<'_>, args: &[Token]) -> fmt::Result {
    let Some((first, rest)) = args.split_first() else {
        return Ok(());
    };
    write!(f, "<{}", shown(first))?;
    rest.iter()
        .try_for_each(|token| write!(f, ", {}", shown(token)))?;
    f.write_str(">")
}

fn shown(token: &Token) -> &str {
    if super::token::is_nameless(token) {
        "…"
    } else {
        token
    }
}
