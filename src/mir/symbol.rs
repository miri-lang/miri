// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The names compiled bodies and data are emitted under.
//!
//! A [`Symbol`] records *what* a name stands for — a function of some module
//! at some type arguments, a method of some owner, a closure lowered out of a
//! body, a GPU kernel, a per-type function codegen emits for the runtime to
//! call through, a datum beside a kernel or a string literal — and
//! [`Symbol::link_name`] is the one place that spells it. MIR lowering and
//! codegen both compose names through it.
//!
//! # The link-name grammar
//!
//! ```text
//! link name   := "main" | c-name | compiler-datum | miri-symbol
//! miri-symbol := root [ "." residency ] "." definition
//! root        := "miri" ( "$" identifier )*             the module declaring a function
//! residency   := "$gpu" ( "$p" position "h" handle )+
//! definition  := item                                  a function
//!              | item "." item                         a method of an owner
//!              | item ".$vtable"                       a vtable
//!              | item ".$" thunk                       a type's drop, decref, clone, compare or equals
//!              | "$" thunk "." encoding                the same for a structural type
//!              | "$lambda" id args                     a lambda
//!              | "$fnref" id args "." target           the thunk of a function reference
//!              | "$nested" id args "." identifier      a nested function
//!              | "$dtor." closure                      a closure's capture destructor
//! item        := identifier args
//! args        := ( "$" token )*
//! ```
//!
//! For example `miri.pick$int` is `pick<int>` and `miri.pick__int` a function
//! written `pick__int`; `miri.Pick._int` is a static method `_int` of `Pick`;
//! `miri.A_b.c` and `miri.A.b_c` are the methods of `A_b` and `A`;
//! `miri.W$int.$drop` and `miri.W__int.$drop` drop a `W<int>` and a `W__int`.
//!
//! The root names the module a top-level function is declared in, one `$`
//! segment per identifier of the path a `use` names that module by: the
//! program's own file adds none, so its `helper` is `miri.helper`, while the
//! `helper` of `local.shapes.circle` is `miri$local$shapes$circle.helper` and
//! the `lattice_unit` of `system.math` is `miri$system$math.lattice_unit`. Two
//! modules may each declare a function of one name, and a call runs the one
//! its name resolved to where the call is written. Every other definition —
//! a type, its methods and thunks, a closure — is linked under the bare
//! `miri` root: type names are one namespace across modules, and a closure is
//! named by the AST node it is written at, which no other node shares.
//!
//! The program's entry point is `main` and a `runtime` function is its C name,
//! verbatim, because code outside this compilation calls them by those names.
//! GPU kernels, the data emitted beside them and string literals keep the
//! compiler's own fixed spellings (`miri_gpu_forall_0`,
//! `__miri_kernel_…_wgsl`, `.miri_str_0_bytes`), which carry no identifier a
//! program wrote.
//!
//! # Why distinct symbols never share a link name
//!
//! An identifier is `[A-Za-z_][A-Za-z0-9_]*`, and a type argument's token is
//! made of `[A-Za-z0-9_-]` only (see [`type_kind_to_mangle_str`], under which
//! two different types never share a token — except the types it has no name
//! for, which all spell [`token::UNSPELLABLE_TYPE_TOKEN`] and are therefore
//! never linked: see [`Symbol::has_an_unnameable_argument`]). Neither contains `.` or `$`, so
//! within a Miri symbol every `.` ends a segment and every `$` starts an
//! argument token or a marker the compiler synthesizes — and a marker can never
//! be mistaken for an identifier, which never starts with `$`. Reading a
//! Miri symbol left to right therefore recovers the symbol: the root, which
//! ends at the first `.` and whose `$` segments, each an identifier of a module
//! path, name the module declaring a function; an optional residency segment
//! (the only segment after the root that starts with `$gpu`); then a
//! definition whose first segment says which kind it is — an identifier for a
//! function, a method, a vtable or a declared type's thunk, told apart by what
//! follows its first `.`; a `$` marker for the rest, each of which either ends
//! there or takes the whole remainder as the one name it carries. Only a
//! function's root carries module segments, so a function of module `a`
//! (`miri$a.b`) is never the method `b` of a type `a` (`miri.a.b`), a function
//! of the program is never one of a module (`miri.b`), and a module path is
//! never read as a function named after one of its segments (`miri$a$b.c`
//! against `miri$a.b`). Every Miri symbol contains a `.`, and neither `main`
//! nor a C name does, so no definition can be linked under the name of the
//! entry point or of a function the runtime library or the C library exports.
//!
//! Two symbols are still compared as values, never through their link names:
//! [`SymbolTable`] refuses a second symbol that spells a name already claimed,
//! as a guard over this grammar rather than a rule a program can meet, and
//! refuses outright a symbol carrying an argument with no name.
//!
//! Type arguments are held as the tokens [`type_kind_to_mangle_str`] gives
//! them, so a symbol is hashable and comparable without comparing [`Type`]s.
//! Two different nameable types never compare equal; two symbols differing
//! only in unnameable arguments do, which is why such a symbol is refused
//! rather than claimed.
//!
//! This guarantee covers link names only. The identifier-only spelling a
//! WGSL module declares a body under ([`Symbol::wgsl_name`]) is not injective,
//! so the bodies GPU code reaches are claimed a second time under that
//! spelling, in a table of their own.

mod table;
pub(crate) mod token;
mod wgsl;
mod written;

pub use table::{Claim, ClaimRefusal, Namespace, SymbolCollision, SymbolTable};

use std::borrow::Cow;
use std::fmt::{self, Write};

use crate::ast::types::Type;
use crate::mir::body::DeviceHandleId;
use crate::type_checker::ModuleId;
use token::type_kind_to_mangle_str;

/// One type argument's token, as [`type_kind_to_mangle_str`] spells it.
type Token = Cow<'static, str>;

/// The first segment of every symbol a Miri definition is linked under.
const ROOT: &str = "miri";

/// Ends each segment of a Miri symbol.
const SEGMENT_SEPARATOR: char = '.';

/// Starts each argument token of a segment and each marker the compiler
/// synthesizes; no identifier or token contains it.
const MARKER: char = '$';

/// Marks the segment naming a specialization's gpu-resident buffers.
const RESIDENCY_MARKER: &str = "gpu";

/// Marks a class's virtual-dispatch table.
const VTABLE_MARKER: &str = "vtable";

/// Marks a lambda's body.
const LAMBDA_MARKER: &str = "lambda";

/// Marks the thunk a reference to a named function is called through.
const FUNCTION_REFERENCE_MARKER: &str = "fnref";

/// Marks a function declared inside another body.
const NESTED_FUNCTION_MARKER: &str = "nested";

/// Marks the destructor releasing a closure's captures.
const DESTRUCTOR_MARKER: &str = "dtor";

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
    /// One pass of a frame statement; every pass of one statement shares the
    /// statement's index.
    FramePass { pass: usize },
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
        module: ModuleId,
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

/// A generic type at some type arguments, as two of its instantiations are
/// told apart: by its name and the token each argument is spelled by in the
/// symbols of the bodies instantiated for it. Ordered, so instantiations
/// discovered in no fixed order can be put in one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeInstance {
    name: String,
    args: Vec<Token>,
}

impl TypeInstance {
    /// The type `name` instantiated at `args`.
    pub fn new<'t>(name: &str, args: impl IntoIterator<Item = &'t Type>) -> Self {
        Self {
            name: name.to_string(),
            args: tokens(args),
        }
    }
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
    /// The function `name` declared at the top level of `module`,
    /// instantiated at `args` when it is generic.
    pub fn function<'t>(
        module: &ModuleId,
        name: &str,
        args: impl IntoIterator<Item = &'t Type>,
    ) -> Self {
        Self::of(SymbolKind::Function {
            module: module.clone(),
            name: name.to_string(),
            args: tokens(args),
        })
    }

    /// The non-generic function `module` declares at the top level as
    /// `name`: the entry point when that is the program's own `main`.
    pub fn declared_function(module: &ModuleId, name: &str) -> Self {
        if *module == ModuleId::Program && name == ENTRY_NAME {
            Self::entry()
        } else {
            Self::function(module, name, &[])
        }
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
    pub fn structural_thunk(kind: ThunkKind, encoding: impl Into<String>) -> Self {
        Self::of(SymbolKind::TypeThunk {
            kind,
            subject: ThunkSubject::Structural(encoding.into()),
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

    /// The method this symbol names when it is a method of `owner` at the
    /// type arguments `owner_args`.
    pub fn method_of<'t>(
        &self,
        owner: &str,
        owner_args: impl IntoIterator<Item = &'t Type>,
    ) -> Option<&str> {
        let SymbolKind::Method {
            owner: own,
            owner_args: own_args,
            method,
            ..
        } = &self.kind
        else {
            return None;
        };
        (own == owner && *own_args == tokens(owner_args)).then_some(method.as_str())
    }

    /// Whether one of this symbol's type arguments has no name of its own:
    /// every type the token grammar cannot name spells alike, so such a symbol
    /// stands for every instantiation at any of them and names none.
    pub fn has_an_unnameable_argument(&self) -> bool {
        let unnameable = |args: &[Token]| {
            args.iter()
                .any(|token| token == token::UNSPELLABLE_TYPE_TOKEN)
        };
        match &self.kind {
            SymbolKind::Function { args, .. }
            | SymbolKind::Vtable { args, .. }
            | SymbolKind::TypeThunk {
                subject: ThunkSubject::Named { args, .. },
                ..
            } => unnameable(args),
            SymbolKind::Method {
                owner_args,
                method_args,
                ..
            } => unnameable(owner_args) || unnameable(method_args),
            SymbolKind::Closure { context_args, .. } => unnameable(context_args),
            SymbolKind::TypeThunk {
                subject: ThunkSubject::Structural(_),
                ..
            }
            | SymbolKind::GpuKernel { .. }
            | SymbolKind::Runtime(_)
            | SymbolKind::Entry
            | SymbolKind::ClosureDestructor(_)
            | SymbolKind::KernelDatum { .. }
            | SymbolKind::StringLiteral { .. } => false,
        }
    }

    /// Whether the runtime library, not this compilation, provides the body.
    pub fn is_runtime(&self) -> bool {
        matches!(self.kind, SymbolKind::Runtime(_))
    }

    /// The name the linker knows this symbol by.
    pub fn link_name(&self) -> String {
        self.to_string()
    }

    /// The name a WGSL module declares this symbol under, spelled with
    /// identifier characters only. A compiler-emitted kernel's entry point is
    /// spelled exactly as the host names it when it launches it.
    pub fn wgsl_name(&self) -> String {
        wgsl::Wgsl(self).to_string()
    }

    /// The module a top-level function is declared in; empty for every
    /// other symbol.
    fn module(&self) -> &[String] {
        match &self.kind {
            SymbolKind::Function { module, .. } => module.path(),
            SymbolKind::Method { .. }
            | SymbolKind::Vtable { .. }
            | SymbolKind::Closure { .. }
            | SymbolKind::GpuKernel { .. }
            | SymbolKind::Runtime(_)
            | SymbolKind::Entry
            | SymbolKind::TypeThunk { .. }
            | SymbolKind::ClosureDestructor(_)
            | SymbolKind::KernelDatum { .. }
            | SymbolKind::StringLiteral { .. } => &[],
        }
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
        match &self.kind {
            SymbolKind::Runtime(c_name) => f.write_str(c_name),
            SymbolKind::Entry => f.write_str(ENTRY_NAME),
            SymbolKind::GpuKernel { .. }
            | SymbolKind::KernelDatum { .. }
            | SymbolKind::StringLiteral { .. } => wgsl::write_kind(f, &self.kind),
            SymbolKind::Function { .. }
            | SymbolKind::Method { .. }
            | SymbolKind::Vtable { .. }
            | SymbolKind::Closure { .. }
            | SymbolKind::TypeThunk { .. }
            | SymbolKind::ClosureDestructor(_) => {
                write_root(f, self.module())?;
                write_residency(f, &self.residency)?;
                f.write_char(SEGMENT_SEPARATOR)?;
                write_definition(f, &self.kind)
            }
        }
    }
}

/// The first segment of a Miri symbol: `miri`, then each identifier of the
/// module a function is declared in: `miri$local$shapes`.
fn write_root(f: &mut fmt::Formatter<'_>, module: &[String]) -> fmt::Result {
    f.write_str(ROOT)?;
    write_arguments(f, module)
}

/// The segments after the root of a symbol for something the program defines
/// or the compiler synthesizes from one.
fn write_definition(f: &mut fmt::Formatter<'_>, kind: &SymbolKind) -> fmt::Result {
    match kind {
        SymbolKind::Function { name, args, .. } => write_item(f, name, args),
        SymbolKind::Method {
            owner,
            owner_args,
            method,
            method_args,
        } => {
            write_item(f, owner, owner_args)?;
            f.write_char(SEGMENT_SEPARATOR)?;
            write_item(f, method, method_args)
        }
        SymbolKind::Vtable { class, args } => {
            write_item(f, class, args)?;
            write_marker_segment(f, VTABLE_MARKER)
        }
        SymbolKind::Closure {
            kind,
            id,
            context_args,
        } => write_closure(f, kind, *id, context_args),
        SymbolKind::TypeThunk { kind, subject } => write_thunk(f, *kind, subject),
        SymbolKind::ClosureDestructor(closure) => {
            write!(f, "{MARKER}{DESTRUCTOR_MARKER}{SEGMENT_SEPARATOR}{closure}")
        }
        SymbolKind::Runtime(_)
        | SymbolKind::Entry
        | SymbolKind::GpuKernel { .. }
        | SymbolKind::KernelDatum { .. }
        | SymbolKind::StringLiteral { .. } => wgsl::write_kind(f, kind),
    }
}

/// An identifier followed by each of its argument tokens: `pick$int$String`.
fn write_item(f: &mut fmt::Formatter<'_>, name: &str, args: &[Token]) -> fmt::Result {
    f.write_str(name)?;
    write_arguments(f, args)
}

/// A trailing segment the compiler synthesizes: `.$vtable`, `.$drop`.
fn write_marker_segment(f: &mut fmt::Formatter<'_>, marker: &str) -> fmt::Result {
    write!(f, "{SEGMENT_SEPARATOR}{MARKER}{marker}")
}

fn write_closure(
    f: &mut fmt::Formatter<'_>,
    kind: &ClosureKind,
    id: usize,
    context_args: &[Token],
) -> fmt::Result {
    let marker = match kind {
        ClosureKind::Lambda => LAMBDA_MARKER,
        ClosureKind::FunctionReference(_) => FUNCTION_REFERENCE_MARKER,
        ClosureKind::NestedFunction(_) => NESTED_FUNCTION_MARKER,
    };
    write!(f, "{MARKER}{marker}{id}")?;
    write_arguments(f, context_args)?;
    match kind {
        ClosureKind::Lambda => Ok(()),
        ClosureKind::FunctionReference(name) | ClosureKind::NestedFunction(name) => {
            write!(f, "{SEGMENT_SEPARATOR}{name}")
        }
    }
}

fn write_thunk(f: &mut fmt::Formatter<'_>, kind: ThunkKind, subject: &ThunkSubject) -> fmt::Result {
    match subject {
        ThunkSubject::Named { name, args } => {
            write_item(f, name, args)?;
            write_marker_segment(f, thunk_marker(kind))
        }
        ThunkSubject::Structural(encoding) => write!(
            f,
            "{MARKER}{}{SEGMENT_SEPARATOR}{encoding}",
            thunk_marker(kind)
        ),
    }
}

fn thunk_marker(kind: ThunkKind) -> &'static str {
    match kind {
        ThunkKind::Drop => "drop",
        ThunkKind::Decref => "decref",
        ThunkKind::Clone => "clone",
        ThunkKind::Compare => "compare",
        ThunkKind::Equals => "equals",
    }
}

/// Each of `segments` after a `$`: a definition's argument tokens, or the
/// identifiers of the module a function is declared in.
fn write_arguments(f: &mut fmt::Formatter<'_>, segments: &[impl AsRef<str>]) -> fmt::Result {
    segments.iter().try_for_each(|segment| {
        f.write_char(MARKER)?;
        f.write_str(segment.as_ref())
    })
}

/// The segment naming the gpu-resident buffers a specialization is for, right
/// after the root: `miri.$gpu$p0h1.scale`. Each gpu-resident argument
/// contributes its position and device handle, so distinct buffers specialize
/// to distinct bodies and one buffer reused across calls maps to one.
///
/// It precedes the definition rather than following it because some
/// definitions end in a name the symbol only carries — the target of a
/// function reference, the closure a destructor releases — and a suffix after
/// one of those could be read as part of it.
fn write_residency(
    f: &mut fmt::Formatter<'_>,
    residency: &[(usize, DeviceHandleId)],
) -> fmt::Result {
    if residency.is_empty() {
        return Ok(());
    }
    write!(f, "{SEGMENT_SEPARATOR}{MARKER}{RESIDENCY_MARKER}")?;
    residency
        .iter()
        .try_for_each(|(position, handle)| write!(f, "{MARKER}p{position}h{}", handle.0))
}

fn tokens<'t>(types: impl IntoIterator<Item = &'t Type>) -> Vec<Token> {
    types
        .into_iter()
        .map(|ty| type_kind_to_mangle_str(&ty.kind))
        .collect()
}
