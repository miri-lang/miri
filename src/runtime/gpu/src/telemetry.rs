// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Cost-model telemetry for GPU residency.
//!
//! Four process-wide counters make the residency cost model observable from
//! Miri source: an
//! upload moves host bytes to a device buffer, a launch dispatches a kernel,
//! a readback copies device bytes back to the host, and a fence is a
//! host-side wait on outstanding device work. The persistent-buffer launch
//! path increments these so a program can assert that a multi-stage pipeline
//! pays exactly one upload + N launches + one readback.
//!
//! Counters are plain atomics with no device dependency, so the accounting
//! is unit-testable without an adapter.

use std::sync::atomic::{AtomicU64, Ordering};

static UPLOADS: AtomicU64 = AtomicU64::new(0);
static LAUNCHES: AtomicU64 = AtomicU64::new(0);
static READBACKS: AtomicU64 = AtomicU64::new(0);
static FENCES: AtomicU64 = AtomicU64::new(0);

pub fn record_upload() {
    UPLOADS.fetch_add(1, Ordering::SeqCst);
}

pub fn record_launch() {
    LAUNCHES.fetch_add(1, Ordering::SeqCst);
}

pub fn record_readback() {
    READBACKS.fetch_add(1, Ordering::SeqCst);
}

pub fn record_fence() {
    FENCES.fetch_add(1, Ordering::SeqCst);
}

pub fn reset() {
    UPLOADS.store(0, Ordering::SeqCst);
    LAUNCHES.store(0, Ordering::SeqCst);
    READBACKS.store(0, Ordering::SeqCst);
    FENCES.store(0, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_reset() {
    reset();
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_uploads() -> u64 {
    UPLOADS.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_launches() -> u64 {
    LAUNCHES.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_readbacks() -> u64 {
    READBACKS.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_fences() -> u64 {
    FENCES.load(Ordering::SeqCst)
}
