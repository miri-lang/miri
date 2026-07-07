// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! High-level math intrinsic operations.
//!
//! These operations are lowered to either CPU libm/Cranelift intrinsics
//! or GPU built-in functions depending on the backend.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathIntrinsic {
    Abs,
    Min,
    Max,
    Pow,
    Sqrt,
    Floor,
    Ceil,
    Round,
    Sin,
    Cos,
    Tan,
    Ln,
    Exp,
    Tanh,
    Exp2,
    Log2,
    Atan2,
    Fract,
    Clamp,
    Mix,
    Smoothstep,
    Step,
    Sign,
    // Vector builtins (GPU-only)
    VecDot,
    VecLength,
    VecNormalize,
    VecCross,
    VecReflect,
    VecMix,
}

impl MathIntrinsic {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "abs" => Some(MathIntrinsic::Abs),
            "min" => Some(MathIntrinsic::Min),
            "max" => Some(MathIntrinsic::Max),
            "pow" => Some(MathIntrinsic::Pow),
            "sqrt" => Some(MathIntrinsic::Sqrt),
            "floor" => Some(MathIntrinsic::Floor),
            "ceil" => Some(MathIntrinsic::Ceil),
            "round" => Some(MathIntrinsic::Round),
            "sin" => Some(MathIntrinsic::Sin),
            "cos" => Some(MathIntrinsic::Cos),
            "tan" => Some(MathIntrinsic::Tan),
            "log" => Some(MathIntrinsic::Ln),
            "exp" => Some(MathIntrinsic::Exp),
            "tanh" => Some(MathIntrinsic::Tanh),
            "exp2" => Some(MathIntrinsic::Exp2),
            "log2" => Some(MathIntrinsic::Log2),
            "atan2" => Some(MathIntrinsic::Atan2),
            "fract" => Some(MathIntrinsic::Fract),
            "clamp" => Some(MathIntrinsic::Clamp),
            "mix" => Some(MathIntrinsic::Mix),
            "smoothstep" => Some(MathIntrinsic::Smoothstep),
            "step" => Some(MathIntrinsic::Step),
            "sign" => Some(MathIntrinsic::Sign),
            _ => None,
        }
    }
}

impl fmt::Display for MathIntrinsic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MathIntrinsic::Abs => "abs",
            MathIntrinsic::Min => "min",
            MathIntrinsic::Max => "max",
            MathIntrinsic::Pow => "pow",
            MathIntrinsic::Sqrt => "sqrt",
            MathIntrinsic::Floor => "floor",
            MathIntrinsic::Ceil => "ceil",
            MathIntrinsic::Round => "round",
            MathIntrinsic::Sin => "sin",
            MathIntrinsic::Cos => "cos",
            MathIntrinsic::Tan => "tan",
            MathIntrinsic::Ln => "ln",
            MathIntrinsic::Exp => "exp",
            MathIntrinsic::Tanh => "tanh",
            MathIntrinsic::Exp2 => "exp2",
            MathIntrinsic::Log2 => "log2",
            MathIntrinsic::Atan2 => "atan2",
            MathIntrinsic::Fract => "fract",
            MathIntrinsic::Clamp => "clamp",
            MathIntrinsic::Mix => "mix",
            MathIntrinsic::Smoothstep => "smoothstep",
            MathIntrinsic::Step => "step",
            MathIntrinsic::Sign => "sign",
            MathIntrinsic::VecDot => "dot",
            MathIntrinsic::VecLength => "length",
            MathIntrinsic::VecNormalize => "normalize",
            MathIntrinsic::VecCross => "cross",
            MathIntrinsic::VecReflect => "reflect",
            MathIntrinsic::VecMix => "mix",
        };
        write!(f, "{}", s)
    }
}
