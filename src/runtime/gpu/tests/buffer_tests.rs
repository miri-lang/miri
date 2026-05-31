// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::buffer::*;
use wgpu::BufferUsages;

#[test]
fn usage_flags_compose_into_wgpu_bitset() {
    let usage = BufferUsage(BufferUsage::STORAGE.0 | BufferUsage::COPY_DST.0);
    let wgpu_usage = usage.to_wgpu();
    assert!(wgpu_usage.contains(BufferUsages::STORAGE));
    assert!(wgpu_usage.contains(BufferUsages::COPY_DST));
}

#[test]
fn elem_count_from_bytes_zero_elem_size_returns_zero() {
    assert_eq!(elem_count_from_bytes(64, 0), 0);
}

#[test]
fn elem_count_from_bytes_divides_by_elem_size() {
    assert_eq!(elem_count_from_bytes(64, 4), 16);
}
