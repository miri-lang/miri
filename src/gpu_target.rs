// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Facts about the GPU target that more than one compiler stage needs.
//!
//! The type checker rejects a `gpu fn` name WGSL reserves and recognizes the
//! atomic builtins a kernel body calls; MIR lowering and the WGSL emitter act on
//! the same two facts. They live here, below every stage, so no stage has to
//! reach into another's internals for them.

use std::fmt;

/// Atomic operation types for GPU memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuAtomicOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Min,
    Max,
    Exchange,
    CompareExchange,
}

impl GpuAtomicOp {
    /// Maps a compiler-recognized atomic builtin function name to its operation.
    /// Returns `None` for any name that is not an atomic builtin.
    pub fn from_builtin_name(name: &str) -> Option<Self> {
        match name {
            "atomic_add" => Some(GpuAtomicOp::Add),
            "atomic_sub" => Some(GpuAtomicOp::Sub),
            "atomic_and" => Some(GpuAtomicOp::And),
            "atomic_or" => Some(GpuAtomicOp::Or),
            "atomic_xor" => Some(GpuAtomicOp::Xor),
            "atomic_min" => Some(GpuAtomicOp::Min),
            "atomic_max" => Some(GpuAtomicOp::Max),
            "atomic_exchange" => Some(GpuAtomicOp::Exchange),
            "atomic_compare_exchange" => Some(GpuAtomicOp::CompareExchange),
            _ => None,
        }
    }
}

impl fmt::Display for GpuAtomicOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuAtomicOp::Add => write!(f, "add"),
            GpuAtomicOp::Sub => write!(f, "sub"),
            GpuAtomicOp::And => write!(f, "and"),
            GpuAtomicOp::Or => write!(f, "or"),
            GpuAtomicOp::Xor => write!(f, "xor"),
            GpuAtomicOp::Min => write!(f, "min"),
            GpuAtomicOp::Max => write!(f, "max"),
            GpuAtomicOp::Exchange => write!(f, "exchange"),
            GpuAtomicOp::CompareExchange => write!(f, "compare_exchange"),
        }
    }
}

/// Why a surface identifier collides with a WGSL reserved form and would be
/// rejected by the shader compiler if emitted verbatim as a WGSL name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgslNameConflict {
    /// WGSL reserves every identifier that begins with two underscores.
    DoubleUnderscorePrefix,
    /// The identifier is a WGSL keyword or reserved word.
    ReservedWord,
}

impl WgslNameConflict {
    /// A human-readable phrase describing the conflict, for a diagnostic that
    /// reads `GPU function name '<name>' <describe()>`.
    pub fn describe(self) -> &'static str {
        match self {
            WgslNameConflict::DoubleUnderscorePrefix => {
                "begins with a reserved double-underscore prefix"
            }
            WgslNameConflict::ReservedWord => "is a reserved WGSL keyword",
        }
    }
}

/// Classifies whether `name` collides with a WGSL reserved form.
///
/// A GPU function name is emitted verbatim as its WGSL entry-point/helper name,
/// so a name WGSL reserves would fail late at shader-module compilation with a
/// generic backend error. Returning the specific conflict lets the type checker
/// reject the name up front with a source-cited diagnostic and a rename hint.
/// Auto-generated kernel names (`miri_gpu_for_*`) never take these forms, so
/// this only fires on user-chosen `gpu fn` names.
pub fn wgsl_name_conflict(name: &str) -> Option<WgslNameConflict> {
    if name.starts_with("__") {
        return Some(WgslNameConflict::DoubleUnderscorePrefix);
    }
    if WGSL_RESERVED_WORDS.contains(&name) {
        return Some(WgslNameConflict::ReservedWord);
    }
    None
}

/// WGSL keywords and reserved words (WGSL specification, §keywords and
/// §reserved-words). Emitting a top-level function with any of these names
/// produces an invalid shader module. Keywords that are also Miri keywords
/// (`fn`, `let`, `var`, `struct`, …) can never reach here — the parser rejects
/// them as function names first — but they are listed for completeness so this
/// slice is the single source of truth for the reserved set.
const WGSL_RESERVED_WORDS: &[&str] = &[
    "NULL",
    "Self",
    "abstract",
    "active",
    "alias",
    "alignas",
    "alignof",
    "as",
    "asm",
    "asm_fragment",
    "async",
    "attribute",
    "auto",
    "await",
    "become",
    "binding_array",
    "break",
    "case",
    "cast",
    "catch",
    "class",
    "co_await",
    "co_return",
    "co_yield",
    "coherent",
    "column_major",
    "common",
    "compile",
    "compile_fragment",
    "concept",
    "const",
    "const_assert",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "continue",
    "continuing",
    "crate",
    "debugger",
    "decltype",
    "default",
    "delete",
    "demote",
    "demote_to_helper",
    "diagnostic",
    "discard",
    "do",
    "dynamic_cast",
    "else",
    "enable",
    "enum",
    "explicit",
    "export",
    "extends",
    "extern",
    "external",
    "fallthrough",
    "false",
    "filter",
    "final",
    "finally",
    "fn",
    "for",
    "friend",
    "from",
    "fxgroup",
    "get",
    "goto",
    "groupshared",
    "highp",
    "if",
    "impl",
    "implements",
    "import",
    "inline",
    "instanceof",
    "interface",
    "layout",
    "let",
    "loop",
    "lowp",
    "macro",
    "macro_rules",
    "match",
    "mediump",
    "meta",
    "mod",
    "module",
    "move",
    "mut",
    "mutable",
    "namespace",
    "new",
    "nil",
    "noexcept",
    "noinline",
    "non_coherent",
    "noncoherent",
    "nointerpolation",
    "noperspective",
    "null",
    "nullptr",
    "of",
    "operator",
    "override",
    "package",
    "packoffset",
    "partition",
    "pass",
    "patch",
    "pixelfragment",
    "precise",
    "precision",
    "premerge",
    "priv",
    "protected",
    "pub",
    "public",
    "readonly",
    "ref",
    "regardless",
    "register",
    "reinterpret_cast",
    "requires",
    "require",
    "resource",
    "restrict",
    "return",
    "self",
    "set",
    "shared",
    "sizeof",
    "smooth",
    "snorm",
    "static",
    "static_assert",
    "static_cast",
    "std",
    "struct",
    "subroutine",
    "super",
    "switch",
    "target",
    "template",
    "this",
    "thread_local",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "typedef",
    "typeid",
    "typename",
    "typeof",
    "union",
    "unless",
    "unorm",
    "unsafe",
    "unsized",
    "use",
    "using",
    "var",
    "varying",
    "virtual",
    "volatile",
    "wgsl",
    "where",
    "while",
    "with",
    "writeonly",
    "yield",
];
