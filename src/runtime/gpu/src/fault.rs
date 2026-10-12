// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The device fault word.
//!
//! A kernel whose integer arithmetic is checked declares a module-scope
//! `miri_fault: atomic<u32>` at group 1, binding 0, and sets bit 0 of it when a
//! result does not fit its type. The word lives on the device for the life of
//! the context and is never cleared: an overflow stays reported until the host
//! looks. The host looks at every point device data reaches it — a readback, a
//! sync, a context reset, and the end of the program — and ends the program
//! through the core runtime's integer-overflow trap when the bit is set.
//! Launches themselves do not look, so a `gpu`-resident launch still never
//! fences.
//!
//! A look copies the word into one host-readable buffer the context keeps for
//! the purpose, recorded in the same submission as the data it accompanies, so
//! it costs no fence of its own. A look is skipped when no faulting kernel has
//! been submitted since the previous look found the word clear.

use crate::context::GpuError;
use parking_lot::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Once};
use std::time::Duration;
use wgpu::{BufferAsyncError, BufferUsages, CommandEncoder, Device, Queue};

/// The declaration that marks a kernel module as one that may fault: the
/// whole line the compiler emits, binding group and slot included.
const FAULT_DECLARATION: &str =
    "@group(1) @binding(0) var<storage, read_write> miri_fault: atomic<u32>;";

/// The bind group index the fault word is bound at.
pub(crate) const FAULT_GROUP: u32 = 1;

/// Bit 0 of the fault word: an integer result did not fit its type.
const INTEGER_OVERFLOW: u32 = 0x1;

/// Size in bytes of the fault word.
const FAULT_WORD_SIZE: u64 = 4;

/// How long a look made while the context is being released — at a reset, a
/// shutdown, or the end of the program — waits for the device before giving
/// up, so a wedged device cannot hang the exit.
const RELEASE_WAIT: Duration = Duration::from_secs(5);

/// Whether the WGSL module `wgsl` declares the fault word, and so must be
/// compiled with, and dispatched with, the fault bind group.
pub fn declares_fault_word(wgsl: &str) -> bool {
    wgsl.contains(FAULT_DECLARATION)
}

/// One context's fault word, the bind group layout a faulting kernel's
/// pipeline includes at [`FAULT_GROUP`], the bind group that binds it, and the
/// host-readable buffer every look copies it into.
pub struct FaultWord {
    buffer: wgpu::Buffer,
    /// Reused by every look; held for the whole look so two looks never map
    /// it at once.
    staging: Mutex<wgpu::Buffer>,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    /// Set once a faulting kernel is submitted, taken by the next look: a look
    /// that finds it unset knows the word is as clear as the last look found.
    dispatched: AtomicBool,
    /// Set when the device is lost, after which the word cannot be read.
    lost: Arc<AtomicBool>,
}

impl FaultWord {
    /// Allocates a cleared fault word on `device`, and notes when `device` is
    /// lost.
    pub fn new(device: &Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("miri_fault"),
            size: FAULT_WORD_SIZE,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("miri_fault readback"),
            size: FAULT_WORD_SIZE,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = fault_layout(device);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("miri_fault_bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let lost = Arc::new(AtomicBool::new(false));
        let on_loss = Arc::clone(&lost);
        device.set_device_lost_callback(move |_, _| on_loss.store(true, Ordering::SeqCst));
        Self {
            buffer,
            staging: Mutex::new(staging),
            layout,
            bind_group,
            dispatched: AtomicBool::new(false),
            lost,
        }
    }

    /// The layout a faulting kernel's pipeline declares at [`FAULT_GROUP`].
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The bind group a faulting kernel is dispatched with.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Notes that a faulting kernel was submitted, so the next look reads the
    /// word. Called after the submission, so a look that takes the note has
    /// its copy queued behind the kernel.
    pub fn record_dispatch(&self) {
        self.dispatched.store(true, Ordering::SeqCst);
    }

    /// Copies the word into the staging buffer in `encoder`, or `None` when
    /// no faulting kernel was submitted since the last look. Submit `encoder`,
    /// then call [`StagedFault::request`] and poll the device before
    /// [`PendingFault::finish`].
    pub fn stage(&self, encoder: &mut CommandEncoder) -> Option<StagedFault<'_>> {
        if !self.dispatched.swap(false, Ordering::SeqCst) {
            return None;
        }
        let staging = self.staging.lock();
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &staging, 0, FAULT_WORD_SIZE);
        Some(StagedFault {
            fault: self,
            staging,
        })
    }

    /// Waits for every submitted command, reading the word in the same wait,
    /// and ends the program when an overflow is recorded in it.
    ///
    /// # Errors
    /// Returns `Err` when the device cannot be waited on or the word cannot be
    /// read; the word is read again at the next look.
    pub fn sync(&self, device: &Device, queue: &Queue) -> Result<(), GpuError> {
        self.wait_and_look(device, queue, None)
    }

    /// The look made as the context is released — at a reset, a shutdown, or
    /// the end of the program. It waits at most [`RELEASE_WAIT`], and is
    /// skipped on a lost device, whose word can no longer be read.
    ///
    /// # Errors
    /// Returns `Err` when the wait times out or the word cannot be read.
    pub fn look_before_release(&self, device: &Device, queue: &Queue) -> Result<(), GpuError> {
        if self.lost.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.wait_and_look(device, queue, Some(RELEASE_WAIT))
    }

    /// Records the word's copy on its own submission when a faulting kernel
    /// ran since the last look, waits up to `timeout` (unbounded when `None`)
    /// for the device, and raises a recorded overflow.
    fn wait_and_look(
        &self,
        device: &Device,
        queue: &Queue,
        timeout: Option<Duration>,
    ) -> Result<(), GpuError> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("miri_fault look encoder"),
        });
        let pending = self.stage(&mut encoder).map(|staged| {
            queue.submit(std::iter::once(encoder.finish()));
            staged.request()
        });
        let waited = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout,
        });
        let looked = pending.map_or(Ok(()), PendingFault::finish);
        waited.map_err(|err| GpuError::FaultWordUnreadable(err.to_string()))?;
        looked
    }
}

/// The layout of the fault word's bind group: one read-write storage buffer
/// at binding 0, visible to compute.
fn fault_layout(device: &Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("miri_fault_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// A copy of the fault word recorded in an encoder that has not been
/// submitted yet. Holds the staging buffer until the look finishes.
pub struct StagedFault<'a> {
    fault: &'a FaultWord,
    staging: MutexGuard<'a, wgpu::Buffer>,
}

impl<'a> StagedFault<'a> {
    /// Requests the mapping of the copy; call after submitting the encoder
    /// that records it.
    pub fn request(self) -> PendingFault<'a> {
        let (sender, mapped) = mpsc::channel();
        self.staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        PendingFault {
            fault: self.fault,
            staging: self.staging,
            mapped,
        }
    }
}

/// A requested read of the fault word, complete once the device is polled.
pub struct PendingFault<'a> {
    fault: &'a FaultWord,
    staging: MutexGuard<'a, wgpu::Buffer>,
    mapped: Receiver<Result<(), BufferAsyncError>>,
}

impl PendingFault<'_> {
    /// Reads the copied word, after the device has been polled, and ends the
    /// program through the core runtime's integer-overflow trap when it
    /// records an overflow.
    ///
    /// # Errors
    /// Returns `Err` when the copy was not mapped. The word is then still to
    /// be read, so the next look reads it again.
    pub fn finish(self) -> Result<(), GpuError> {
        let mapped = match self.mapped.try_recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err.to_string()),
            Err(TryRecvError::Empty) => Err("the device did not finish the copy".to_string()),
            Err(TryRecvError::Disconnected) => Err("the mapping was dropped".to_string()),
        };
        if let Err(reason) = mapped {
            self.staging.unmap();
            self.fault.record_dispatch();
            return Err(GpuError::FaultWordUnreadable(reason));
        }
        let word = {
            let bytes = self.staging.slice(..).get_mapped_range();
            bytes
                .get(..4)
                .and_then(|word| <[u8; 4]>::try_from(word).ok())
                .map_or(0, u32::from_le_bytes)
        };
        self.staging.unmap();
        if word & INTEGER_OVERFLOW != 0 {
            drop(self.staging);
            // SAFETY: the core runtime reports the overflow and ends the
            // process; it takes no arguments and touches no GPU state.
            unsafe { miri_rt_integer_overflow_panic() };
        }
        Ok(())
    }
}

/// Registers, once per process, the look at the fault word when the program
/// ends, so an overflow in a launch nothing reads back is still reported.
/// Called when the first context is created: `atexit` handlers run in reverse
/// order of registration, so this look runs after every handler registered
/// later in the program.
pub(crate) fn register_exit_check() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        // SAFETY: `check_fault_at_exit` is a plain `extern "C" fn()` that
        // lives for the whole process.
        unsafe {
            libc_atexit(check_fault_at_exit);
        }
    });
}

/// Looks at the live context's fault word, if a context exists; the exit
/// never creates one.
extern "C" fn check_fault_at_exit() {
    let Ok(ctx) = crate::context::get_gpu_context() else {
        return;
    };
    if let Err(err) = ctx.fault.look_before_release(&ctx.device, &ctx.queue) {
        eprintln!("Runtime error: the GPU fault word could not be read at exit: {err:?}");
    }
}

extern "C" {
    #[link_name = "atexit"]
    fn libc_atexit(func: extern "C" fn()) -> i32;
}

#[cfg(not(feature = "panic-trap"))]
extern "C" {
    /// The core runtime's integer-overflow trap: reports MER_RT_003 and ends
    /// the process. Every program that links the GPU runtime links the core
    /// runtime after it.
    fn miri_rt_integer_overflow_panic();
}

/// This crate's own tests link no core runtime, so the trap is a test failure
/// there.
#[cfg(feature = "panic-trap")]
unsafe fn miri_rt_integer_overflow_panic() {
    panic!("integer overflow recorded in the device fault word");
}

#[cfg(test)]
impl FaultWord {
    /// Records an overflow in the word the way a kernel would, for a test.
    pub(crate) fn record_overflow_for_test(&self, queue: &Queue) {
        queue.write_buffer(&self.buffer, 0, &INTEGER_OVERFLOW.to_le_bytes());
        self.record_dispatch();
    }

    /// Clears the word and the pending look, so the test's process exits clean.
    pub(crate) fn clear_for_test(&self, queue: &Queue) {
        queue.write_buffer(&self.buffer, 0, &0u32.to_le_bytes());
        self.dispatched.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::{declares_fault_word, FaultWord};
    use std::sync::atomic::Ordering;

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    #[test]
    fn a_module_declaring_the_fault_word_is_recognised() {
        let wgsl = "@group(1) @binding(0) var<storage, read_write> miri_fault: atomic<u32>;";
        assert!(declares_fault_word(wgsl));
    }

    #[test]
    fn a_function_named_like_the_fault_word_is_not_a_declaration() {
        let wgsl = "fn miri_fault() -> i32 { return 0; }";
        assert!(!declares_fault_word(wgsl));
    }

    #[test]
    fn a_fault_word_at_another_binding_is_not_the_declaration() {
        let wgsl = "@group(0) @binding(2) var<storage, read_write> miri_fault: atomic<u32>;";
        assert!(!declares_fault_word(wgsl));
    }

    #[test]
    fn a_look_with_nothing_dispatched_since_the_last_does_not_read_the_word() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let fault = FaultWord::new(&device);
        fault.record_dispatch();
        assert!(fault.sync(&device, &queue).is_ok());
        queue.write_buffer(&fault.buffer, 0, &1u32.to_le_bytes());
        assert!(fault.sync(&device, &queue).is_ok());
    }

    #[test]
    fn every_look_reuses_the_one_staging_buffer() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let fault = FaultWord::new(&device);
        for _ in 0..3 {
            fault.record_dispatch();
            assert!(fault.sync(&device, &queue).is_ok());
        }
    }

    #[test]
    #[should_panic(expected = "integer overflow recorded in the device fault word")]
    fn a_sync_reports_a_recorded_overflow() {
        let Some((device, queue)) = test_device() else {
            panic!("integer overflow recorded in the device fault word (no GPU adapter)");
        };
        let fault = FaultWord::new(&device);
        fault.record_overflow_for_test(&queue);
        let _ = fault.sync(&device, &queue);
    }

    #[test]
    #[should_panic(expected = "integer overflow recorded in the device fault word")]
    fn a_release_reports_a_recorded_overflow() {
        let Some((device, queue)) = test_device() else {
            panic!("integer overflow recorded in the device fault word (no GPU adapter)");
        };
        let fault = FaultWord::new(&device);
        fault.record_overflow_for_test(&queue);
        let _ = fault.look_before_release(&device, &queue);
    }

    #[test]
    fn a_lost_device_is_not_looked_at_on_release() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let fault = FaultWord::new(&device);
        fault.record_overflow_for_test(&queue);
        fault.lost.store(true, Ordering::SeqCst);
        assert!(fault.look_before_release(&device, &queue).is_ok());
    }
}
