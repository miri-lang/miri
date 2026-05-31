// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::context::*;

#[test]
fn miri_gpu_init_is_pure() {
    let _ = miri_gpu_init();
}

#[test]
fn miri_gpu_is_available_matches_context_presence() {
    // The two functions must agree: `is_available` is the contract
    // exposed to Miri source via `system.gpu.is_gpu_available()`.
    let observed = miri_gpu_is_available();
    let actual_presence = u8::from(GPU_CONTEXT.get().is_some());
    assert_eq!(
        observed, actual_presence,
        "is_available must mirror GPU_CONTEXT state without reinitializing"
    );
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
