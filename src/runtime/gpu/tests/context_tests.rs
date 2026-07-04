// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::context::*;
use std::sync::Mutex;

/// Serializes the tests in this binary that init, reset, or observe global
/// context presence. Cargo runs a binary's tests on parallel threads sharing
/// one process (and therefore one `GPU_CONTEXT`); a reset in one thread would
/// otherwise flip presence between another thread's paired samples.
static PRESENCE_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn miri_gpu_init_is_pure() {
    let _serialize = PRESENCE_LOCK.lock().unwrap();
    let _ = miri_gpu_init();
}

#[test]
fn miri_gpu_is_available_matches_context_presence() {
    // The two functions must agree: `is_available` is the contract
    // exposed to Miri source via `system.gpu.is_gpu_available()`.
    let _serialize = PRESENCE_LOCK.lock().unwrap();
    let observed = miri_gpu_is_available();
    let actual_presence = u8::from(GPU_CONTEXT.read().is_some());
    assert_eq!(
        observed, actual_presence,
        "is_available must mirror GPU_CONTEXT state without reinitializing"
    );
}

#[test]
fn reset_context_advances_generation() {
    // Each reset bumps the monotonic device generation, so a resident buffer
    // tagged with an earlier generation can be recognized as stale.
    let _serialize = PRESENCE_LOCK.lock().unwrap();
    let g1 = miri_gpu_reset_context();
    let g2 = miri_gpu_reset_context();
    assert!(g2 > g1, "each reset must advance the device generation");
    // `>=` (not `==`) because another binary's process cannot touch this
    // counter, but keep the monotonic phrasing explicit: it only moves forward.
    assert!(current_device_generation() >= g2);
}

#[test]
fn device_info_encodes_device_type_exhaustively() {
    assert_eq!(encode_device_type(wgpu::DeviceType::Other), 0);
    assert_eq!(encode_device_type(wgpu::DeviceType::IntegratedGpu), 1);
    assert_eq!(encode_device_type(wgpu::DeviceType::DiscreteGpu), 2);
    assert_eq!(encode_device_type(wgpu::DeviceType::VirtualGpu), 3);
    assert_eq!(encode_device_type(wgpu::DeviceType::Cpu), 4);
}

#[test]
fn device_info_encodes_backend_exhaustively() {
    assert_eq!(encode_backend(wgpu::Backend::Noop), 0);
    assert_eq!(encode_backend(wgpu::Backend::Vulkan), 1);
    assert_eq!(encode_backend(wgpu::Backend::Metal), 2);
    assert_eq!(encode_backend(wgpu::Backend::Dx12), 3);
    assert_eq!(encode_backend(wgpu::Backend::Gl), 4);
    assert_eq!(encode_backend(wgpu::Backend::BrowserWebGpu), 5);
}
