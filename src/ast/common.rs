// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::Expression;

/// Visibility level for class/struct members and declarations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum MemberVisibility {
    #[default]
    Public,
    Protected,
    Private,
}

/// Represents the properties of a function declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FunctionProperties {
    pub is_async: bool,
    pub is_parallel: bool,
    pub is_gpu: bool,
    pub visibility: MemberVisibility,
}

/// Represents a parameter in a function declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Parameter {
    pub name: String,
    pub typ: Box<Expression>,
    pub guard: Option<Box<Expression>>,
    pub default_value: Option<Box<Expression>>,
    pub is_out: bool,
}

/// Known runtime targets for runtime function declarations.
///
/// Each variant maps to a separate runtime library that provides
/// `#[no_mangle] extern "C"` FFI functions linked into the final binary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeKind {
    /// The core runtime (`miri-runtime-core`), providing string, allocation,
    /// I/O, and collection primitives.
    Core,
}

impl RuntimeKind {
    /// Parses a runtime name string into a `RuntimeKind`.
    ///
    /// Returns `None` if the name does not match any known runtime.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "core" => Some(Self::Core),
            _ => None,
        }
    }

    /// Returns the string name of this runtime kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Core => "core",
        }
    }

    /// Returns the static library name (without `lib` prefix or extension)
    /// used for `-l` linker flags.
    pub fn library_name(&self) -> &'static str {
        match self {
            Self::Core => "miri_runtime_core",
        }
    }
}
