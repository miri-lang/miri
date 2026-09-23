// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Persistent device buffers keyed by a binding's `DeviceHandleId`.
//!
//! A `gpu`-resident binding (`gpu let` / `gpu var`) carries a stable handle
//! id assigned at MIR lowering. That id names the *binding*, not one buffer:
//! every execution of the declaration opens a fresh activation of the handle
//! (`miri_gpu_acquire`), and the activation's scope exit closes it
//! (`miri_gpu_release`). A recursive call re-declares the binding while its
//! caller's activation is still live, and a loop re-declares it once per
//! iteration; each gets a buffer of its own. Activations of one handle nest
//! strictly — they follow the call stack — so the live activation is the top
//! of a per-thread stack, and every other entry point (launch, readback,
//! upload) resolves the handle through it.
//!
//! An activation's device buffer is allocated on the first kernel launch that
//! captures it and then survives across every later launch in the same
//! activation: the second and subsequent launches reuse the resident buffer,
//! paying no upload and no fence.
//!
//! The table fails closed. Every handle the compiler hands a launch belongs to
//! a declaration that acquired it (a `gpu let` / `gpu var`, or a reduction's
//! output buffer); a borrowed parameter reaches its caller's live activation.
//! So a handle with no open activation owns no buffer: nothing is stored
//! under it, and a release of it frees nothing. Such a release means the
//! compiled code's acquire/release pairing went wrong; it is recorded, and the
//! next launch on this thread refuses to run
//! ([`check_activation_balance`]) rather than proceed on bookkeeping it can no
//! longer trust.
//!
//! What the table cannot see is an activation that was opened and never
//! closed (a callee that missed its release): its key stays on top of the
//! handle's stack, and the caller's later use resolves to it. Ruling that out
//! needs the activation key itself to travel with the binding as a value,
//! rather than being looked up from the static id.
//!
//! Handle id `0` is the sentinel for "no handle" — a host-resident capture
//! that is uploaded transiently and read back after every launch, matching
//! the pre-residency behavior.

use crate::context::GpuError;
use crate::wire::WireConversion;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use wgpu::Buffer;

/// Sentinel handle id for a host-resident (non-persistent) capture.
pub const HOST_HANDLE: u64 = 0;

/// Activation keys are drawn from the top half of the id space. Compile-time
/// handle ids count up from `1`, so an activation key never collides with the
/// id of a handle that was never acquired.
const FIRST_ACTIVATION_KEY: u64 = 1 << 63;

static NEXT_ACTIVATION_KEY: AtomicU64 = AtomicU64::new(FIRST_ACTIVATION_KEY);

thread_local! {
    /// Live activations of each acquired handle, innermost last.
    static ACTIVATIONS: RefCell<HashMap<u64, Vec<u64>>> = RefCell::new(HashMap::new());
    /// The handle of the first release that found no open activation, held
    /// until the next launch reports it.
    static UNBALANCED_RELEASE: Cell<Option<u64>> = const { Cell::new(None) };
}

struct ResidentBuffer {
    buffer: Buffer,
    byte_len: usize,
    /// How the buffer's elements were converted from their host width on
    /// upload, and so how they convert back on readback.
    conversion: WireConversion,
    /// Device generation at allocation time. A buffer whose generation no
    /// longer matches the current one belongs to a device that has been reset
    /// (device loss or adapter change) and must not be bound again.
    generation: u64,
}

static DEVICE_BUFFERS: Lazy<RwLock<HashMap<u64, ResidentBuffer>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Returns a clone of the resident buffer handle for `handle_id` paired with
/// its device byte length and element conversion, or `None` when nothing has
/// been allocated for its live activation yet (or it has none).
pub fn resident_buffer(handle_id: u64) -> Option<(Buffer, usize, WireConversion)> {
    let key = active_key(handle_id)?;
    let current = crate::context::current_device_generation();
    DEVICE_BUFFERS
        .read()
        .get(&key)
        .filter(|entry| entry.generation == current)
        .map(|entry| (entry.buffer.clone(), entry.byte_len, entry.conversion))
}

/// Records `buffer` as the persistent device buffer of `handle_id`'s live
/// activation, tagged with the current device generation so a later reset can
/// recognize it as stale. `conversion` records how its elements were converted
/// on upload, so a readback or a later explicit upload converts them the same
/// way.
///
/// # Errors
/// Refuses a handle with no open activation: no scope would ever release a
/// buffer stored for it.
pub fn insert_resident(
    handle_id: u64,
    buffer: Buffer,
    byte_len: usize,
    conversion: WireConversion,
) -> Result<(), GpuError> {
    let key = active_key(handle_id).ok_or_else(|| {
        GpuError::InactiveHandle(format!(
            "gpu binding {handle_id} was captured with no live activation"
        ))
    })?;
    DEVICE_BUFFERS.write().insert(
        key,
        ResidentBuffer {
            buffer,
            byte_len,
            conversion,
            generation: crate::context::current_device_generation(),
        },
    );
    Ok(())
}

/// Removes every resident buffer. Called by `miri_gpu_reset_context` when the
/// device is dropped: the buffers reference the old device and cannot be used
/// on the replacement.
pub fn clear_resident() {
    DEVICE_BUFFERS.write().clear();
}

/// The device-table key `handle_id` currently resolves to: its innermost live
/// activation, or `None` when it has none.
pub fn active_key(handle_id: u64) -> Option<u64> {
    ACTIVATIONS.with(|activations| {
        activations
            .borrow()
            .get(&handle_id)
            .and_then(|stack| stack.last().copied())
    })
}

/// Opens a fresh activation of `handle_id`, hiding any enclosing activation's
/// buffer until this one is released. The host sentinel has no activations.
pub fn acquire(handle_id: u64) {
    if handle_id == HOST_HANDLE {
        return;
    }
    let key = NEXT_ACTIVATION_KEY.fetch_add(1, Ordering::Relaxed);
    ACTIVATIONS.with(|activations| {
        activations
            .borrow_mut()
            .entry(handle_id)
            .or_default()
            .push(key);
    });
}

/// Closes the innermost activation of `handle_id` and returns its key, or
/// `None` when it has no live activation.
fn close_activation(handle_id: u64) -> Option<u64> {
    ACTIVATIONS.with(|activations| {
        let mut activations = activations.borrow_mut();
        let stack = activations.get_mut(&handle_id)?;
        let key = stack.pop();
        if stack.is_empty() {
            activations.remove(&handle_id);
        }
        key
    })
}

/// Releases the device buffer of the innermost activation of a binding that
/// left scope, and closes that activation. Returns `true` when a buffer was
/// present and removed; only an actual removal is recorded in the release
/// telemetry counter, so an activation that never launched does not inflate
/// the count.
///
/// A release with no open activation frees nothing and is recorded for
/// [`check_activation_balance`]; the host sentinel never has one and is
/// ignored.
pub fn release(handle_id: u64) -> bool {
    if handle_id == HOST_HANDLE {
        return false;
    }
    let Some(key) = close_activation(handle_id) else {
        record_unbalanced_release(handle_id);
        return false;
    };
    let removed = DEVICE_BUFFERS.write().remove(&key).is_some();
    if removed {
        crate::telemetry::record_release();
    }
    removed
}

/// Remembers the first release on this thread that found no open activation.
fn record_unbalanced_release(handle_id: u64) {
    UNBALANCED_RELEASE.with(|pending| {
        if pending.get().is_none() {
            pending.set(Some(handle_id));
        }
    });
}

/// Reports, once, a release on this thread that found no open activation.
///
/// # Errors
/// Returns the imbalance when one was recorded since the last check, so the
/// launch that asks refuses to run through the runtime's launch-failure exit.
pub fn check_activation_balance() -> Result<(), GpuError> {
    match UNBALANCED_RELEASE.with(Cell::take) {
        None => Ok(()),
        Some(handle_id) => Err(GpuError::InactiveHandle(format!(
            "gpu binding {handle_id} was released with no live activation"
        ))),
    }
}

/// Opens a fresh activation of a `gpu`-resident binding. The compiler emits
/// this at every `gpu let` / `gpu var` declaration.
///
/// # Safety
/// Safe to call with any value; `handle_id` is an opaque key.
#[no_mangle]
pub extern "C" fn miri_gpu_acquire(handle_id: u64) {
    // A declaration is the earliest point at which a program's GPU use is
    // observable from outside the process. Reporting here is what lets a run
    // that declared a gpu binding and never launched anything be told apart
    // from one that never reached the GPU at all: the first reports zeros, the
    // second reports nothing.
    crate::telemetry::report();
    acquire(handle_id);
}

/// Frees the device buffer of a `gpu`-resident binding's innermost activation
/// and closes it. The compiler emits this at the binding's scope exit.
///
/// # Safety
/// Safe to call with any value; `handle_id` is an opaque key.
#[no_mangle]
pub extern "C" fn miri_gpu_release(handle_id: u64) {
    crate::telemetry::report();
    let _ = release(handle_id);
}

#[cfg(test)]
mod tests {
    use super::{release, HOST_HANDLE};

    /// Releasing a handle that was never allocated a buffer — a `gpu` local
    /// never captured by a launch — returns `false` and therefore does not record a release. Uses a
    /// handle id that no test uploads to, so it never races a resident buffer.
    #[test]
    fn release_of_absent_handle_is_a_noop() {
        assert!(!release(u64::MAX));
        assert!(!release(HOST_HANDLE));
    }
}
