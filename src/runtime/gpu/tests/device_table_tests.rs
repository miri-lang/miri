// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::buffer::{BufferUsage, GpuBuffer};
use miri_runtime_gpu::context::{miri_gpu_init, miri_gpu_reset_context};
use miri_runtime_gpu::device_table::*;

#[test]
fn release_of_absent_handle_is_a_noop() {
    assert!(!release(u64::MAX));
}

#[test]
fn reset_context_invalidates_resident_buffers() {
    // Recovery contract: after a device reset a buffer that was resident on the
    // previous device is neither returned nor reused on the replacement device.
    if miri_gpu_init() == 0 {
        eprintln!("no GPU adapter; skipping reset_context_invalidates_resident_buffers");
        return;
    }
    // A dedicated handle no other test uploads to, so parallel tests never race it.
    let handle = 0xF340_0000_0001u64;
    let buffer = GpuBuffer::new(16, BufferUsage::STORAGE, 4)
        .expect("adapter present, buffer allocation should succeed")
        .buffer;
    insert_resident(handle, buffer, 16, false);
    assert!(
        resident_buffer(handle).is_some(),
        "buffer must be resident before the reset"
    );

    let _ = miri_gpu_reset_context();

    assert!(
        resident_buffer(handle).is_none(),
        "a resident buffer from the pre-reset device must not be reused"
    );
}
