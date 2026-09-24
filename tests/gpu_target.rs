// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU-target facts shared across compiler stages.

use miri::gpu_target::{wgsl_name_conflict, GpuAtomicOp, WgslNameConflict};

#[test]
fn every_atomic_builtin_name_maps_to_its_operation() {
    let names = [
        ("atomic_add", GpuAtomicOp::Add),
        ("atomic_sub", GpuAtomicOp::Sub),
        ("atomic_and", GpuAtomicOp::And),
        ("atomic_or", GpuAtomicOp::Or),
        ("atomic_xor", GpuAtomicOp::Xor),
        ("atomic_min", GpuAtomicOp::Min),
        ("atomic_max", GpuAtomicOp::Max),
        ("atomic_exchange", GpuAtomicOp::Exchange),
        ("atomic_compare_exchange", GpuAtomicOp::CompareExchange),
    ];
    for (name, op) in names {
        assert_eq!(GpuAtomicOp::from_builtin_name(name), Some(op), "{name}");
    }
    assert_eq!(GpuAtomicOp::from_builtin_name("atomic_load"), None);
}

#[test]
fn wgsl_reserved_forms_are_classified() {
    assert_eq!(
        wgsl_name_conflict("__helper"),
        Some(WgslNameConflict::DoubleUnderscorePrefix)
    );
    assert_eq!(
        wgsl_name_conflict("alias"),
        Some(WgslNameConflict::ReservedWord)
    );
    assert_eq!(wgsl_name_conflict("blur_step"), None);
}
