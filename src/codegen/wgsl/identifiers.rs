// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Spelling of source-derived names in the emitted WGSL.
//!
//! A buffer, scalar capture or `shared` array keeps its Miri name in the
//! shader wherever that name is safe, so the WGSL stays readable. A name is
//! unsafe when WGSL reserves it, when it would shadow a predeclared WGSL type,
//! enumerant or built-in function, or when it could clash with a name the
//! emitter synthesizes. Every synthesized name begins with `_` (`_inputs`,
//! `_global_id`, `_3`, …) apart from the subgroup built-in parameters, so any
//! source name beginning with `_` is treated as unsafe. An unsafe name is
//! emitted as [`ESCAPE_PREFIX`] followed by the name.
//!
//! The mapping is injective: a safe name never begins with `_`, while every
//! escaped name does, and prefixing preserves distinctness among escaped names.
//! No synthesized name begins with [`ESCAPE_PREFIX`], so an escaped name cannot
//! meet one either.

use crate::mir::backend::gpu::wgsl_name_conflict;

/// Prefix of an escaped source name. It must begin with `_` (the injectivity
/// argument above relies on it) and no synthesized name may begin with it.
const ESCAPE_PREFIX: &str = "_src_";

/// Names the emitter synthesizes that do not begin with `_`: the subgroup
/// built-in parameters of a kernel entry point.
pub(super) const SUBGROUP_SIZE: &str = "SUBGROUP_SIZE";
pub(super) const SUBGROUP_INVOCATION_ID: &str = "SUBGROUP_INVOCATION_ID";

/// WGSL predeclared types and type generators, enumerants and built-in
/// functions, plus the extension scalars the native backend accepts. None is a
/// reserved word, but a module-scope declaration of one shadows it, so a buffer
/// named `min` would break every `min(..)` call in the kernel.
const WGSL_PREDECLARED: &[&str] = &[
    "bool",
    "i32",
    "u32",
    "f32",
    "f16",
    "array",
    "atomic",
    "vec2",
    "vec3",
    "vec4",
    "mat2x2",
    "mat2x3",
    "mat2x4",
    "mat3x2",
    "mat3x3",
    "mat3x4",
    "mat4x2",
    "mat4x3",
    "mat4x4",
    "ptr",
    "sampler",
    "sampler_comparison",
    "texture_1d",
    "texture_2d",
    "texture_2d_array",
    "texture_3d",
    "texture_cube",
    "texture_cube_array",
    "texture_multisampled_2d",
    "texture_depth_multisampled_2d",
    "texture_external",
    "texture_storage_1d",
    "texture_storage_2d",
    "texture_storage_2d_array",
    "texture_storage_3d",
    "texture_depth_2d",
    "texture_depth_2d_array",
    "texture_depth_cube",
    "texture_depth_cube_array",
    "read",
    "write",
    "read_write",
    "function",
    "private",
    "workgroup",
    "uniform",
    "storage",
    "rgba8unorm",
    "rgba8snorm",
    "rgba8uint",
    "rgba8sint",
    "rgba16unorm",
    "rgba16snorm",
    "rgba16uint",
    "rgba16sint",
    "rgba16float",
    "rg8unorm",
    "rg8snorm",
    "rg8uint",
    "rg8sint",
    "rg16unorm",
    "rg16snorm",
    "rg16uint",
    "rg16sint",
    "rg16float",
    "r32uint",
    "r32sint",
    "r32float",
    "rg32uint",
    "rg32sint",
    "rg32float",
    "rgba32uint",
    "rgba32sint",
    "rgba32float",
    "bgra8unorm",
    "r8unorm",
    "r8snorm",
    "r8uint",
    "r8sint",
    "r16unorm",
    "r16snorm",
    "r16uint",
    "r16sint",
    "r16float",
    "rgb10a2unorm",
    "rgb10a2uint",
    "rg11b10ufloat",
    "bitcast",
    "all",
    "any",
    "select",
    "arrayLength",
    "abs",
    "acos",
    "acosh",
    "asin",
    "asinh",
    "atan",
    "atanh",
    "atan2",
    "ceil",
    "clamp",
    "cos",
    "cosh",
    "countLeadingZeros",
    "countOneBits",
    "countTrailingZeros",
    "cross",
    "degrees",
    "determinant",
    "distance",
    "dot",
    "dot4U8Packed",
    "dot4I8Packed",
    "exp",
    "exp2",
    "extractBits",
    "faceForward",
    "firstLeadingBit",
    "firstTrailingBit",
    "floor",
    "fma",
    "fract",
    "frexp",
    "insertBits",
    "inverseSqrt",
    "ldexp",
    "length",
    "log",
    "log2",
    "max",
    "min",
    "mix",
    "modf",
    "normalize",
    "pow",
    "quantizeToF16",
    "radians",
    "reflect",
    "refract",
    "reverseBits",
    "round",
    "saturate",
    "sign",
    "sin",
    "sinh",
    "smoothstep",
    "sqrt",
    "step",
    "tan",
    "tanh",
    "transpose",
    "trunc",
    "dpdx",
    "dpdxCoarse",
    "dpdxFine",
    "dpdy",
    "dpdyCoarse",
    "dpdyFine",
    "fwidth",
    "fwidthCoarse",
    "fwidthFine",
    "textureDimensions",
    "textureGather",
    "textureGatherCompare",
    "textureLoad",
    "textureNumLayers",
    "textureNumLevels",
    "textureNumSamples",
    "textureSample",
    "textureSampleBias",
    "textureSampleCompare",
    "textureSampleCompareLevel",
    "textureSampleGrad",
    "textureSampleLevel",
    "textureSampleBaseClampToEdge",
    "textureStore",
    "atomicLoad",
    "atomicStore",
    "atomicAdd",
    "atomicSub",
    "atomicMax",
    "atomicMin",
    "atomicAnd",
    "atomicOr",
    "atomicXor",
    "atomicExchange",
    "atomicCompareExchangeWeak",
    "pack4x8snorm",
    "pack4x8unorm",
    "pack4xI8",
    "pack4xU8",
    "pack4xI8Clamp",
    "pack4xU8Clamp",
    "pack2x16snorm",
    "pack2x16unorm",
    "pack2x16float",
    "unpack4x8snorm",
    "unpack4x8unorm",
    "unpack4xI8",
    "unpack4xU8",
    "unpack2x16snorm",
    "unpack2x16unorm",
    "unpack2x16float",
    "storageBarrier",
    "textureBarrier",
    "workgroupBarrier",
    "workgroupUniformLoad",
    "subgroupAdd",
    "subgroupExclusiveAdd",
    "subgroupInclusiveAdd",
    "subgroupAll",
    "subgroupAnd",
    "subgroupAny",
    "subgroupBallot",
    "subgroupBroadcast",
    "subgroupBroadcastFirst",
    "subgroupElect",
    "subgroupMax",
    "subgroupMin",
    "subgroupMul",
    "subgroupExclusiveMul",
    "subgroupInclusiveMul",
    "subgroupOr",
    "subgroupShuffle",
    "subgroupShuffleDown",
    "subgroupShuffleUp",
    "subgroupShuffleXor",
    "subgroupXor",
    "quadBroadcast",
    "quadSwapDiagonal",
    "quadSwapX",
    "quadSwapY",
    "i64",
    "u64",
    "f64",
    "push_constant",
    "r64uint",
];

/// The WGSL identifier for a source-derived `name`: the name itself when it is
/// safe to emit, otherwise the escaped form. Characters WGSL does not admit in
/// an identifier are replaced by `_` first.
pub fn source_identifier(name: &str) -> String {
    let sanitized = sanitize(name);
    if is_unsafe(&sanitized) {
        format!("{ESCAPE_PREFIX}{sanitized}")
    } else {
        sanitized
    }
}

fn is_unsafe(name: &str) -> bool {
    name.starts_with('_')
        || name == SUBGROUP_SIZE
        || name == SUBGROUP_INVOCATION_ID
        || wgsl_name_conflict(name).is_some()
        || WGSL_PREDECLARED.contains(&name)
}

fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}
