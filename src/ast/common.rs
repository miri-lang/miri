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
    /// The GPU runtime (`miri-runtime-gpu`), providing the wgpu/WebGPU host
    /// driver for `gpu fn` and `gpu for` constructs.
    Gpu,
}

impl RuntimeKind {
    /// Parses a runtime name string into a `RuntimeKind`.
    ///
    /// Returns `None` if the name does not match any known runtime.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "core" => Some(Self::Core),
            "gpu" => Some(Self::Gpu),
            _ => None,
        }
    }

    /// Returns the string name of this runtime kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Gpu => "gpu",
        }
    }

    /// Returns the static library name (without `lib` prefix or extension)
    /// used for `-l` linker flags.
    pub fn library_name(&self) -> &'static str {
        match self {
            Self::Core => "miri_runtime_core",
            Self::Gpu => "miri_runtime_gpu",
        }
    }

    /// Extra linker arguments required by a runtime's transitive
    /// dependencies. Kept on the enum rather than in `pipeline.rs` so
    /// that adding a new runtime does not force a pipeline edit.
    pub fn extra_link_args(&self) -> &'static [&'static str] {
        match self {
            Self::Core => &[],
            Self::Gpu => {
                if cfg!(target_os = "macos") {
                    &[
                        "-framework",
                        "Foundation",
                        "-framework",
                        "CoreFoundation",
                        "-framework",
                        "CoreGraphics",
                        "-framework",
                        "CoreText",
                        "-framework",
                        "AppKit",
                        "-framework",
                        "Metal",
                        "-framework",
                        "MetalKit",
                        "-framework",
                        "QuartzCore",
                        "-framework",
                        "IOKit",
                        "-framework",
                        "IOSurface",
                    ]
                } else {
                    // Linux: naga/wgpu/hexf pull in libm transcendentals
                    // (`log2f`, `exp2`, `sin`, ...) that the system linker
                    // does not resolve implicitly the way macOS does.
                    &["-lm"]
                }
            }
        }
    }
}
