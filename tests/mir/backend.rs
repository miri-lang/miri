// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::TypeKind;
use miri::mir::backend::{GpuIndexNarrowing, I32_INDEX_MAX};

/// A 64-bit index must saturate into the non-negative 32-bit range before the
/// cast, so a value >= 2^31 cannot wrap into an aliasing in-bounds index.
#[test]
fn i64_index_saturates_to_i32() {
    assert_eq!(
        GpuIndexNarrowing::from_index_kind(&TypeKind::I64),
        GpuIndexNarrowing::SaturateToI32
    );
}

/// An `Int` index is already 32-bit and needs only an identity 32-bit cast.
#[test]
fn int_index_is_identity() {
    assert_eq!(
        GpuIndexNarrowing::from_index_kind(&TypeKind::Int),
        GpuIndexNarrowing::Identity
    );
}

/// Any other index scalar type needs no narrowing.
#[test]
fn other_index_needs_no_narrowing() {
    assert_eq!(
        GpuIndexNarrowing::from_index_kind(&TypeKind::F32),
        GpuIndexNarrowing::None
    );
    assert_eq!(
        GpuIndexNarrowing::from_index_kind(&TypeKind::U64),
        GpuIndexNarrowing::None
    );
}

/// The saturation upper bound is the largest signed 32-bit value, matching the
/// `clamp(..., 0, 2147483647)` the WGSL emitter renders.
#[test]
fn i32_index_max_is_signed_max() {
    assert_eq!(I32_INDEX_MAX, 2147483647);
}
