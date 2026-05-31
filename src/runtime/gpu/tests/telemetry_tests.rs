// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::telemetry::*;
use parking_lot::Mutex;

// The counters are process-global; serialize the tests that mutate them
// so concurrent test threads don't observe each other's increments.
static GUARD: Mutex<()> = Mutex::new(());

#[test]
fn reset_zeroes_every_counter() {
    let _g = GUARD.lock();
    record_upload();
    record_launch();
    record_readback();
    record_fence();
    reset();
    assert_eq!(miri_gpu_telemetry_uploads(), 0);
    assert_eq!(miri_gpu_telemetry_launches(), 0);
    assert_eq!(miri_gpu_telemetry_readbacks(), 0);
    assert_eq!(miri_gpu_telemetry_fences(), 0);
}

#[test]
fn each_record_increments_its_own_counter() {
    let _g = GUARD.lock();
    reset();
    record_upload();
    record_launch();
    record_launch();
    record_readback();
    assert_eq!(miri_gpu_telemetry_uploads(), 1);
    assert_eq!(miri_gpu_telemetry_launches(), 2);
    assert_eq!(miri_gpu_telemetry_readbacks(), 1);
    assert_eq!(miri_gpu_telemetry_fences(), 0);
}
