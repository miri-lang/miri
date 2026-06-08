// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Persistent device buffers keyed by a binding's `DeviceHandleId`.
//!
//! A `gpu`-resident binding (`gpu let` / `gpu var`) carries a stable handle
//! id assigned at MIR lowering. Its device buffer is allocated on the first
//! kernel launch that captures it and then survives across every later launch
//! that captures the same handle: the second and
//! subsequent launches reuse the resident buffer, paying no upload and no
//! fence. The buffer is released (via `miri_gpu_release`) when its handle is
//! re-declared — the compiler emits a reset at every `gpu let` / `gpu var` so
//! a binding re-entered in a repeated call starts fresh; otherwise the buffer
//! lives until process teardown drops the table.
//!
//! Handle id `0` is the sentinel for "no handle" — a host-resident capture
//! that is uploaded transiently and read back after every launch, matching
//! the pre-residency behavior.

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use wgpu::Buffer;

/// Sentinel handle id for a host-resident (non-persistent) capture.
pub const HOST_HANDLE: u64 = 0;

struct ResidentBuffer {
    buffer: Buffer,
    byte_len: usize,
    /// True if this buffer's host data was narrowed (i64→i32) on upload.
    needs_widen: bool,
}

static DEVICE_BUFFERS: Lazy<RwLock<HashMap<u64, ResidentBuffer>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Returns a clone of the resident buffer handle for `handle_id` paired with
/// its uploaded byte length and narrowing flag, or `None` when nothing has been allocated for it yet.
pub fn resident_buffer(handle_id: u64) -> Option<(Buffer, usize, bool)> {
    DEVICE_BUFFERS
        .read()
        .get(&handle_id)
        .map(|entry| (entry.buffer.clone(), entry.byte_len, entry.needs_widen))
}

/// Records `buffer` as the persistent device buffer for `handle_id`.
/// `needs_widen` tracks whether the buffer was narrowed on upload and needs widening on readback.
pub fn insert_resident(handle_id: u64, buffer: Buffer, byte_len: usize, needs_widen: bool) {
    DEVICE_BUFFERS.write().insert(
        handle_id,
        ResidentBuffer {
            buffer,
            byte_len,
            needs_widen,
        },
    );
}

/// Releases the device buffer owned by a dropped host-side binding. Returns
/// `true` when a buffer was present and removed.
pub fn release(handle_id: u64) -> bool {
    DEVICE_BUFFERS.write().remove(&handle_id).is_some()
}

/// # Safety
/// Safe to call with any value; `handle_id` is an opaque key.
#[no_mangle]
pub extern "C" fn miri_gpu_release(handle_id: u64) {
    let _ = release(handle_id);
}
