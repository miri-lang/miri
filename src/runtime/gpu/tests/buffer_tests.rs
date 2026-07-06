// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::buffer::*;
use std::ptr;
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

#[test]
fn buffer_from_data_with_null_data_returns_null() {
    unsafe {
        let result = miri_gpu_buffer_from_data(ptr::null(), 64, 1, 4);
        assert!(
            result.is_null(),
            "null data pointer should return null handle"
        );
    }
}

#[test]
fn buffer_from_data_with_zero_size_returns_null() {
    unsafe {
        let dummy_data = [0u8; 4];
        let result = miri_gpu_buffer_from_data(dummy_data.as_ptr(), 0, 1, 4);
        assert!(result.is_null(), "zero size should return null handle");
    }
}

#[test]
fn buffer_write_with_null_handle_returns_zero() {
    unsafe {
        let result = miri_gpu_buffer_write(ptr::null(), 0, ptr::null(), 0);
        assert_eq!(result, 0, "null handle should return 0 (failure)");
    }
}

#[test]
fn buffer_write_with_null_data_returns_zero() {
    unsafe {
        let dummy_handle = GpuBufferHandle {
            id: 1,
            size: 64,
            elem_size: 4,
            elem_count: 16,
        };
        let result = miri_gpu_buffer_write(&dummy_handle, 0, ptr::null(), 64);
        assert_eq!(result, 0, "null data pointer should return 0 (failure)");
    }
}

#[test]
fn buffer_read_with_null_handle_returns_zero() {
    unsafe {
        let mut out = [0u8; 64];
        let result = miri_gpu_buffer_read(ptr::null(), out.as_mut_ptr(), 64);
        assert_eq!(result, 0, "null handle should return 0 (failure)");
    }
}

#[test]
fn buffer_read_with_null_out_returns_zero() {
    unsafe {
        let dummy_handle = GpuBufferHandle {
            id: 1,
            size: 64,
            elem_size: 4,
            elem_count: 16,
        };
        let result = miri_gpu_buffer_read(&dummy_handle, ptr::null_mut(), 64);
        assert_eq!(result, 0, "null out pointer should return 0 (failure)");
    }
}

#[test]
fn buffer_size_with_null_handle_returns_zero() {
    unsafe {
        let result = miri_gpu_buffer_size(ptr::null());
        assert_eq!(result, 0, "null handle should return size 0");
    }
}

#[test]
fn buffer_elem_count_with_null_handle_returns_zero() {
    unsafe {
        let result = miri_gpu_buffer_elem_count(ptr::null());
        assert_eq!(result, 0, "null handle should return elem_count 0");
    }
}

#[test]
fn buffer_copy_with_null_src_handle_returns_zero() {
    unsafe {
        let dummy_dst = GpuBufferHandle {
            id: 2,
            size: 64,
            elem_size: 4,
            elem_count: 16,
        };
        let result = miri_gpu_buffer_copy(ptr::null(), 0, &dummy_dst, 0, 64);
        assert_eq!(result, 0, "null src handle should return 0 (failure)");
    }
}

#[test]
fn buffer_copy_with_null_dst_handle_returns_zero() {
    unsafe {
        let dummy_src = GpuBufferHandle {
            id: 1,
            size: 64,
            elem_size: 4,
            elem_count: 16,
        };
        let result = miri_gpu_buffer_copy(&dummy_src, 0, ptr::null(), 0, 64);
        assert_eq!(result, 0, "null dst handle should return 0 (failure)");
    }
}

#[test]
fn buffer_free_with_null_handle_is_noop() {
    unsafe {
        // Should not panic or crash.
        miri_gpu_buffer_free(ptr::null_mut());
    }
}
