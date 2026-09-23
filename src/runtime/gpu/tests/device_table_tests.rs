// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::buffer::{BufferUsage, GpuBuffer};
use miri_runtime_gpu::context::{miri_gpu_init, miri_gpu_reset_context, GpuError};
use miri_runtime_gpu::device_table::*;
use miri_runtime_gpu::wire::WireConversion;
use std::sync::{Mutex, MutexGuard};

/// The resident-buffer table is process-global, and a context reset clears
/// every entry in it. Tests that hold a resident buffer across assertions, and
/// the test that resets the context, take this lock so a parallel reset cannot
/// drop a buffer another test is still checking.
static RESIDENT_TABLE: Mutex<()> = Mutex::new(());

fn lock_resident_table() -> MutexGuard<'static, ()> {
    RESIDENT_TABLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn release_of_absent_handle_is_a_noop() {
    assert!(!release(u64::MAX));
    let _ = check_activation_balance();
}

#[test]
fn reset_context_invalidates_resident_buffers() {
    let _table = lock_resident_table();
    // Recovery contract: after a device reset a buffer that was resident on the
    // previous device is neither returned nor reused on the replacement device.
    if miri_gpu_init() == 0 {
        eprintln!("no GPU adapter; skipping reset_context_invalidates_resident_buffers");
        return;
    }
    // A dedicated handle no other test uploads to, so parallel tests never race it.
    let handle = 0xF340_0000_0001u64;
    acquire(handle);
    insert_resident(handle, storage_buffer(), 16, WireConversion::Identity)
        .expect("an acquired handle holds a buffer");
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

fn storage_buffer() -> wgpu::Buffer {
    GpuBuffer::new(16, BufferUsage::STORAGE, 4)
        .expect("adapter present, buffer allocation should succeed")
        .buffer
}

#[test]
fn unacquired_handle_resolves_to_no_activation() {
    let handle = 0xA000_0000_0001u64;
    assert_eq!(active_key(handle), None);
}

/// A launch that captures a handle with no open activation has no buffer it
/// may own; keying one on the bare id would let it outlive every scope.
#[test]
fn buffer_for_an_unacquired_handle_is_refused() {
    if miri_gpu_init() == 0 {
        eprintln!("no GPU adapter; skipping buffer_for_an_unacquired_handle_is_refused");
        return;
    }
    let handle = 0xA000_0000_0004u64;
    assert!(matches!(
        insert_resident(handle, storage_buffer(), 16, WireConversion::Identity),
        Err(GpuError::InactiveHandle(_))
    ));
    assert!(resident_buffer(handle).is_none());
}

/// A release with no open activation must free nothing — not the bare id's
/// entry, not another activation's buffer — and is reported to the next
/// launch, which refuses to run on a table whose bookkeeping went wrong.
#[test]
fn release_without_an_open_activation_frees_nothing_and_is_reported() {
    let _table = lock_resident_table();
    if miri_gpu_init() == 0 {
        eprintln!("no GPU adapter; skipping release_without_an_open_activation_frees_nothing_and_is_reported");
        return;
    }
    let owner = 0xA000_0000_0005u64;
    let stray = 0xA000_0000_0006u64;
    acquire(owner);
    insert_resident(owner, storage_buffer(), 16, WireConversion::Identity)
        .expect("an acquired handle holds a buffer");
    assert!(check_activation_balance().is_ok());

    assert!(!release(stray), "a stray release frees nothing");
    assert!(
        resident_buffer(owner).is_some(),
        "another binding's live buffer survives a stray release"
    );
    assert!(matches!(
        check_activation_balance(),
        Err(GpuError::InactiveHandle(_))
    ));
    assert!(
        check_activation_balance().is_ok(),
        "the imbalance is reported once"
    );

    assert!(release(owner));
    assert!(
        !release(owner),
        "a second release of a closed activation frees nothing"
    );
    assert!(check_activation_balance().is_err());
}

#[test]
fn release_of_the_host_sentinel_is_not_an_imbalance() {
    assert!(!release(HOST_HANDLE));
    assert!(check_activation_balance().is_ok());
}

#[test]
fn each_acquire_opens_a_fresh_activation_and_release_restores_the_caller() {
    let handle = 0xA000_0000_0002u64;
    acquire(handle);
    let outer = active_key(handle);
    acquire(handle);
    let inner = active_key(handle);
    assert!(outer.is_some(), "an acquired handle has an activation");
    assert_ne!(
        outer,
        Some(handle),
        "an activation must not reuse the binding's static id"
    );
    assert_ne!(
        inner, outer,
        "a nested activation must not share its caller's buffer"
    );

    assert!(
        !release(handle),
        "the inner activation never allocated a buffer"
    );
    assert_eq!(
        active_key(handle),
        outer,
        "release must hand the caller its activation back"
    );
    assert!(!release(handle));
    assert_eq!(active_key(handle), None);
    assert!(
        check_activation_balance().is_ok(),
        "balanced releases are not an imbalance"
    );
}

#[test]
fn acquire_of_the_host_sentinel_opens_no_activation() {
    acquire(HOST_HANDLE);
    assert_eq!(active_key(HOST_HANDLE), None);
}

#[test]
fn nested_activation_hides_the_callers_resident_buffer() {
    let _table = lock_resident_table();
    if miri_gpu_init() == 0 {
        eprintln!("no GPU adapter; skipping nested_activation_hides_the_callers_resident_buffer");
        return;
    }
    let handle = 0xA000_0000_0003u64;
    acquire(handle);
    insert_resident(handle, storage_buffer(), 16, WireConversion::Identity)
        .expect("an acquired handle holds a buffer");

    acquire(handle);
    assert!(
        resident_buffer(handle).is_none(),
        "the inner activation starts empty"
    );
    assert!(!release(handle));

    assert!(
        resident_buffer(handle).is_some(),
        "the caller's buffer survives the callee"
    );
    assert!(release(handle), "the caller's release frees its own buffer");
    assert!(resident_buffer(handle).is_none());
}
