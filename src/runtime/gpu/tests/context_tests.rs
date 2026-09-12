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

/// Requests an adapter independently of the runtime, so the assertions below
/// compare `miri_gpu_is_available` against ground truth rather than against
/// itself. Mirrors the runtime's own selection: default instance options plus
/// whatever `WGPU_BACKEND` pins.
fn an_adapter_is_reachable() -> bool {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    if let Some(backends) = wgpu::Backends::from_env() {
        descriptor.backends = backends;
    }
    let instance = wgpu::Instance::new(descriptor);
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .is_ok()
}

#[test]
fn is_available_answers_for_the_adapter_not_the_context() {
    // The predicate is what Miri source sees as `system.gpu.is_gpu_available()`.
    // It must report whether a device *can* be created, so a program that asks
    // before launching anything gets the same answer as one that asks after.
    let _serialize = PRESENCE_LOCK.lock().unwrap();
    miri_gpu_reset_context();
    let observed = miri_gpu_is_available();
    assert_eq!(
        observed,
        u8::from(an_adapter_is_reachable()),
        "is_available must reflect adapter reachability, not whether a device \
        happens to have been created yet"
    );
}

#[test]
fn is_available_answers_without_creating_a_device() {
    // Checking for a GPU must not cost one. A device created by the probe
    // would also outlive the answer, holding the adapter for the process.
    let _serialize = PRESENCE_LOCK.lock().unwrap();
    miri_gpu_reset_context();
    let _ = miri_gpu_is_available();
    assert!(
        GPU_CONTEXT.read().is_none(),
        "probing availability must leave no device behind"
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
