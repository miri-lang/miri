// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU buffer allocation, transfer, and FFI handles.

use crate::context::{get_gpu_context, GpuError};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use wgpu::{Buffer, BufferUsages};

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) static BUFFER_REGISTRY: Lazy<RwLock<HashMap<u64, Arc<GpuBuffer>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferUsage(pub u32);

impl BufferUsage {
    pub const STORAGE: Self = Self(1);
    pub const UNIFORM: Self = Self(2);
    pub const VERTEX: Self = Self(4);
    pub const INDEX: Self = Self(8);
    pub const COPY_SRC: Self = Self(16);
    pub const COPY_DST: Self = Self(32);
    pub const MAP_READ: Self = Self(64);
    pub const MAP_WRITE: Self = Self(128);

    fn to_wgpu(self) -> BufferUsages {
        let mut usage = BufferUsages::empty();
        if self.0 & Self::STORAGE.0 != 0 {
            usage |= BufferUsages::STORAGE;
        }
        if self.0 & Self::UNIFORM.0 != 0 {
            usage |= BufferUsages::UNIFORM;
        }
        if self.0 & Self::VERTEX.0 != 0 {
            usage |= BufferUsages::VERTEX;
        }
        if self.0 & Self::INDEX.0 != 0 {
            usage |= BufferUsages::INDEX;
        }
        if self.0 & Self::COPY_SRC.0 != 0 {
            usage |= BufferUsages::COPY_SRC;
        }
        if self.0 & Self::COPY_DST.0 != 0 {
            usage |= BufferUsages::COPY_DST;
        }
        if self.0 & Self::MAP_READ.0 != 0 {
            usage |= BufferUsages::MAP_READ;
        }
        if self.0 & Self::MAP_WRITE.0 != 0 {
            usage |= BufferUsages::MAP_WRITE;
        }
        usage
    }
}

pub struct GpuBuffer {
    pub id: u64,
    pub buffer: Buffer,
    pub size: u64,
    pub usage: BufferUsage,
    pub elem_size: usize,
    pub elem_count: usize,
}

#[repr(C)]
pub struct GpuBufferHandle {
    pub id: u64,
    pub size: u64,
    pub elem_size: usize,
    pub elem_count: usize,
}

impl GpuBuffer {
    pub fn new(size: u64, usage: BufferUsage, elem_size: usize) -> Result<Self, GpuError> {
        let ctx = get_gpu_context()?;
        let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Miri GPU Buffer"),
            size,
            usage: usage.to_wgpu(),
            mapped_at_creation: false,
        });
        let id = NEXT_BUFFER_ID.fetch_add(1, Ordering::SeqCst);
        let elem_count = elem_count_from_bytes(size as usize, elem_size);
        Ok(Self {
            id,
            buffer,
            size,
            usage,
            elem_size,
            elem_count,
        })
    }

    pub fn from_data(data: &[u8], usage: BufferUsage, elem_size: usize) -> Result<Self, GpuError> {
        let ctx = get_gpu_context()?;
        let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Miri GPU Buffer"),
            size: data.len() as u64,
            usage: usage.to_wgpu() | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.queue.write_buffer(&buffer, 0, data);
        let id = NEXT_BUFFER_ID.fetch_add(1, Ordering::SeqCst);
        let elem_count = elem_count_from_bytes(data.len(), elem_size);
        Ok(Self {
            id,
            buffer,
            size: data.len() as u64,
            usage,
            elem_size,
            elem_count,
        })
    }

    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), GpuError> {
        let ctx = get_gpu_context()?;
        ctx.queue.write_buffer(&self.buffer, offset, data);
        Ok(())
    }

    /// Reads buffer contents back to host memory via a staging buffer.
    pub fn read(&self) -> Result<Vec<u8>, GpuError> {
        let ctx = get_gpu_context()?;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Buffer"),
            size: self.size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Read Encoder"),
            });
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &staging, 0, self.size);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .map_err(|_| GpuError::BufferCreationFailed)?
            .map_err(|_| GpuError::BufferCreationFailed)?;

        let data = slice.get_mapped_range().to_vec();
        staging.unmap();
        Ok(data)
    }
}

fn elem_count_from_bytes(bytes: usize, elem_size: usize) -> usize {
    bytes.checked_div(elem_size).unwrap_or(0)
}

pub fn get_buffer(id: u64) -> Option<Arc<GpuBuffer>> {
    BUFFER_REGISTRY.read().get(&id).cloned()
}

fn register_buffer(buffer: GpuBuffer) -> Arc<GpuBuffer> {
    let id = buffer.id;
    let arc = Arc::new(buffer);
    BUFFER_REGISTRY.write().insert(id, Arc::clone(&arc));
    arc
}

fn remove_buffer(id: u64) {
    BUFFER_REGISTRY.write().remove(&id);
}

fn handle_from(buffer: &GpuBuffer) -> GpuBufferHandle {
    GpuBufferHandle {
        id: buffer.id,
        size: buffer.size,
        elem_size: buffer.elem_size,
        elem_count: buffer.elem_count,
    }
}

#[no_mangle]
pub extern "C" fn miri_gpu_buffer_new(
    size: u64,
    usage: u32,
    elem_size: usize,
) -> *mut GpuBufferHandle {
    match GpuBuffer::new(size, BufferUsage(usage), elem_size) {
        Ok(buffer) => {
            let handle = handle_from(&buffer);
            register_buffer(buffer);
            Box::into_raw(Box::new(handle))
        }
        Err(_) => ptr::null_mut(),
    }
}

/// # Safety
/// `data` must point to at least `size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_from_data(
    data: *const u8,
    size: usize,
    usage: u32,
    elem_size: usize,
) -> *mut GpuBufferHandle {
    if data.is_null() || size == 0 {
        return ptr::null_mut();
    }
    let slice = std::slice::from_raw_parts(data, size);
    match GpuBuffer::from_data(slice, BufferUsage(usage), elem_size) {
        Ok(buffer) => {
            let handle = handle_from(&buffer);
            register_buffer(buffer);
            Box::into_raw(Box::new(handle))
        }
        Err(_) => ptr::null_mut(),
    }
}

/// # Safety
/// `handle` must be a valid `GpuBufferHandle` pointer and `data` must
/// point to at least `size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_write(
    handle: *const GpuBufferHandle,
    offset: u64,
    data: *const u8,
    size: usize,
) -> u8 {
    if handle.is_null() || data.is_null() {
        return 0;
    }
    let id = (*handle).id;
    let Some(buffer) = get_buffer(id) else {
        return 0;
    };
    let slice = std::slice::from_raw_parts(data, size);
    u8::from(buffer.write(offset, slice).is_ok())
}

/// # Safety
/// `handle` must be a valid `GpuBufferHandle` pointer and `out` must
/// point to at least `max_size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_read(
    handle: *const GpuBufferHandle,
    out: *mut u8,
    max_size: usize,
) -> u8 {
    if handle.is_null() || out.is_null() {
        return 0;
    }
    let id = (*handle).id;
    let Some(buffer) = get_buffer(id) else {
        return 0;
    };
    match buffer.read() {
        Ok(data) => {
            let len = data.len().min(max_size);
            ptr::copy_nonoverlapping(data.as_ptr(), out, len);
            1
        }
        Err(_) => 0,
    }
}

/// # Safety
/// `handle` must be a valid `GpuBufferHandle` pointer.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_size(handle: *const GpuBufferHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    (*handle).size
}

/// # Safety
/// `handle` must be a valid `GpuBufferHandle` pointer.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_elem_count(handle: *const GpuBufferHandle) -> usize {
    if handle.is_null() {
        return 0;
    }
    (*handle).elem_count
}

/// # Safety
/// Both `src_handle` and `dst_handle` must be valid `GpuBufferHandle`
/// pointers.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_copy(
    src_handle: *const GpuBufferHandle,
    src_offset: u64,
    dst_handle: *const GpuBufferHandle,
    dst_offset: u64,
    size: u64,
) -> u8 {
    if src_handle.is_null() || dst_handle.is_null() {
        return 0;
    }
    let Some(src) = get_buffer((*src_handle).id) else {
        return 0;
    };
    let Some(dst) = get_buffer((*dst_handle).id) else {
        return 0;
    };
    let Ok(ctx) = get_gpu_context() else {
        return 0;
    };
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Copy Encoder"),
        });
    encoder.copy_buffer_to_buffer(&src.buffer, src_offset, &dst.buffer, dst_offset, size);
    ctx.queue.submit(std::iter::once(encoder.finish()));
    1
}

/// # Safety
/// `handle` must be a valid `GpuBufferHandle` pointer previously
/// returned by `miri_gpu_buffer_new` or `miri_gpu_buffer_from_data`.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_buffer_free(handle: *mut GpuBufferHandle) {
    if !handle.is_null() {
        let id = (*handle).id;
        remove_buffer(id);
        let _ = Box::from_raw(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
