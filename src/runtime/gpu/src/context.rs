//! GPU context: instance, adapter, device, queue.
//!
//! A single process-wide `GpuContext` is held in a `OnceCell`. All
//! buffer / kernel FFI functions look it up via `get_gpu_context`.

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::sync::Arc;
use wgpu::{Adapter, Device, Features, Instance, Queue};

static GPU_CONTEXT: OnceCell<Arc<GpuContext>> = OnceCell::new();

#[derive(Debug, Clone)]
pub enum GpuError {
    NoAdapter,
    DeviceCreationFailed(String),
    NotInitialized,
    BufferCreationFailed,
    ShaderCompilationFailed(String),
    KernelNotFound(String),
    InvalidDimensions,
    /// A kernel referenced a scalar type (e.g. WGSL `i64`/`u64`/`f64`) that
    /// the active adapter does not advertise via the matching wgpu feature
    /// (`SHADER_INT64` / `SHADER_F64`). The compiler cannot silently widen or
    /// truncate widths because that corrupts host/device buffer round-trips,
    /// so the launch is refused with this error before submission.
    UnsupportedScalar(String),
}

#[repr(C)]
#[derive(Clone)]
pub struct GpuDeviceInfo {
    pub name: [u8; 256],
    pub name_len: usize,
    pub vendor_id: u32,
    pub device_id: u32,
    /// 0=Other, 1=IntegratedGpu, 2=DiscreteGpu, 3=VirtualGpu, 4=Cpu.
    pub device_type: u8,
    /// 0=Empty, 1=Vulkan, 2=Metal, 3=Dx12, 4=Gl, 5=BrowserWebGpu.
    pub backend: u8,
    pub max_buffer_size: u64,
    pub max_workgroup_size: u32,
    pub max_workgroups_x: u32,
    pub max_workgroups_y: u32,
    pub max_workgroups_z: u32,
}

impl Default for GpuDeviceInfo {
    fn default() -> Self {
        Self {
            name: [0; 256],
            name_len: 0,
            vendor_id: 0,
            device_id: 0,
            device_type: 0,
            backend: 0,
            max_buffer_size: 0,
            max_workgroup_size: 0,
            max_workgroups_x: 0,
            max_workgroups_y: 0,
            max_workgroups_z: 0,
        }
    }
}

pub struct GpuContext {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    pub info: RwLock<GpuDeviceInfo>,
    /// Subset of `OPTIONAL_SHADER_FEATURES` actually granted by the adapter
    /// at device creation. Cached so launch sites can refuse kernels that
    /// reference unsupported scalars without re-querying the device.
    pub enabled_shader_features: Features,
}

/// Optional wgpu features the runtime tries to enable when the adapter
/// reports them as supported. Only features that change the set of WGSL
/// scalars a kernel may use go in here, so the launch-site gate stays
/// focused on type-level correctness.
fn optional_shader_features() -> Features {
    Features::SHADER_INT64 | Features::SHADER_F64
}

impl GpuContext {
    pub fn new() -> Result<Self, GpuError> {
        let instance = Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|_| GpuError::NoAdapter)?;

        let required_shader_features = optional_shader_features() & adapter.features();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Miri GPU Device"),
            required_features: required_shader_features,
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| GpuError::DeviceCreationFailed(err.to_string()))?;

        let enabled_shader_features = device.features() & optional_shader_features();
        let info = build_device_info(&adapter);
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            info: RwLock::new(info),
            enabled_shader_features,
        })
    }

    pub fn get_info(&self) -> GpuDeviceInfo {
        self.info.read().clone()
    }
}

fn build_device_info(adapter: &Adapter) -> GpuDeviceInfo {
    let adapter_info = adapter.get_info();
    let limits = adapter.limits();

    let mut info = GpuDeviceInfo::default();
    let name_bytes = adapter_info.name.as_bytes();
    let name_len = name_bytes.len().min(255);
    info.name[..name_len].copy_from_slice(&name_bytes[..name_len]);
    info.name_len = name_len;
    info.vendor_id = adapter_info.vendor;
    info.device_id = adapter_info.device;
    info.device_type = encode_device_type(adapter_info.device_type);
    info.backend = encode_backend(adapter_info.backend);
    info.max_buffer_size = limits.max_buffer_size;
    info.max_workgroup_size = limits.max_compute_invocations_per_workgroup;
    info.max_workgroups_x = limits.max_compute_workgroups_per_dimension;
    info.max_workgroups_y = limits.max_compute_workgroups_per_dimension;
    info.max_workgroups_z = limits.max_compute_workgroups_per_dimension;
    info
}

fn encode_device_type(device_type: wgpu::DeviceType) -> u8 {
    match device_type {
        wgpu::DeviceType::Other => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::DiscreteGpu => 2,
        wgpu::DeviceType::VirtualGpu => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

fn encode_backend(backend: wgpu::Backend) -> u8 {
    match backend {
        wgpu::Backend::Noop => 0,
        wgpu::Backend::Vulkan => 1,
        wgpu::Backend::Metal => 2,
        wgpu::Backend::Dx12 => 3,
        wgpu::Backend::Gl => 4,
        wgpu::Backend::BrowserWebGpu => 5,
    }
}

pub fn get_gpu_context() -> Result<&'static Arc<GpuContext>, GpuError> {
    GPU_CONTEXT.get().ok_or(GpuError::NotInitialized)
}

/// `pub(crate)` so the inline launch path can lazily initialize the
/// process-wide `GPU_CONTEXT` instead of holding its own. Keeping a single
/// `OnceCell` is what makes `miri_gpu_is_available()` reflect the actual
/// state after a `gpu for` dispatch.
pub(crate) fn init_gpu_context() -> Result<&'static Arc<GpuContext>, GpuError> {
    GPU_CONTEXT.get_or_try_init(|| GpuContext::new().map(Arc::new))
}

/// Initializes the GPU runtime. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn miri_gpu_init() -> u8 {
    match init_gpu_context() {
        Ok(_) => 1,
        Err(err) => {
            log::error!("GPU init failed: {:?}", err);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn miri_gpu_is_available() -> u8 {
    u8::from(GPU_CONTEXT.get().is_some())
}

/// # Safety
/// `out` must be a valid writable pointer to `GpuDeviceInfo`.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_get_info(out: *mut GpuDeviceInfo) -> u8 {
    if out.is_null() {
        return 0;
    }
    match get_gpu_context() {
        Ok(ctx) => {
            *out = ctx.get_info();
            1
        }
        Err(_) => 0,
    }
}

/// Copies the adapter name into `out` (up to `max_len` bytes). Returns
/// the number of bytes written, or 0 on error.
///
/// # Safety
/// `out` must be a valid writable buffer of at least `max_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_get_device_name(out: *mut u8, max_len: usize) -> usize {
    if out.is_null() || max_len == 0 {
        return 0;
    }
    match get_gpu_context() {
        Ok(ctx) => {
            let info = ctx.info.read();
            let len = info.name_len.min(max_len);
            std::ptr::copy_nonoverlapping(info.name.as_ptr(), out, len);
            len
        }
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn miri_gpu_max_buffer_size() -> u64 {
    match get_gpu_context() {
        Ok(ctx) => ctx.info.read().max_buffer_size,
        Err(_) => 0,
    }
}

/// Blocks until all submitted GPU work is complete.
#[no_mangle]
pub extern "C" fn miri_gpu_sync() {
    if let Ok(ctx) = get_gpu_context() {
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

/// Drains pending work. The context itself outlives the call because
/// `OnceCell` has no removal API; reuse remains safe.
#[no_mangle]
pub extern "C" fn miri_gpu_shutdown() {
    if let Ok(ctx) = get_gpu_context() {
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
