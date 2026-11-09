// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod config;

use miri::ast::types::TypeKind;
use miri::mir::backend::GpuIndexNarrowing;

/// A 64-bit index is computed in the device's 32-bit lane, so it needs only a
/// 32-bit cast; a negative one is left for the index check to report rather
/// than clamped into the first element.
#[test]
fn i64_index_is_identity() {
    assert_eq!(
        GpuIndexNarrowing::from_index_kind(&TypeKind::I64),
        GpuIndexNarrowing::Identity
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
