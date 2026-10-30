// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What the profiler asks of the operating system: a sampler that records
//! the main thread's stack at a rate of CPU time, the bounds of that
//! thread's stack, and the names of the symbols a record's addresses fall in.
//!
//! Linux samples from a `SIGPROF` handler that a timer on the process CPU
//! clock signals on the main thread alone (see [`super::signal_sampler`]).
//! macOS has neither per-process POSIX timers nor thread-directed delivery,
//! and its process-wide `ITIMER_PROF` delivers far fewer signals than asked
//! for, so it samples from a thread of its own instead, by the main thread's
//! CPU time alone (see [`super::mach_sampler`]).

use super::frame_walk::StackBounds;

/// Whether this target is one whose sampled registers the sampler can read.
pub(crate) const IS_SUPPORTED: bool = cfg!(all(
    any(target_os = "macos", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
));

/// Start sampling the calling thread, the program's main thread, `hz` times
/// per second of CPU time. `Err` says what could not be set up.
#[cfg(target_os = "linux")]
pub(crate) fn start(hz: u32) -> Result<(), &'static str> {
    super::signal_sampler::start(hz)
}

/// Start sampling the calling thread, the program's main thread, `hz` times
/// per second of its CPU time. `Err` says what could not be set up.
#[cfg(target_os = "macos")]
pub(crate) fn start(hz: u32) -> Result<(), &'static str> {
    if super::mach_sampler::start(hz) {
        Ok(())
    } else {
        Err("the profiling thread could not be started")
    }
}

/// No sampler on a target the profiler does not support.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn start(_hz: u32) -> Result<(), &'static str> {
    Err("this target has no sampling profiler")
}

/// Stop sampling; no sample is recorded once this returns.
#[cfg(target_os = "linux")]
pub(crate) fn stop() {
    super::signal_sampler::stop();
}

/// Stop sampling; no sample is recorded once this returns, unless the
/// sampler thread failed to finish within its time limit.
#[cfg(target_os = "macos")]
pub(crate) fn stop() {
    super::mach_sampler::stop();
}

/// No sampler to stop on a target the profiler does not support.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn stop() {}

/// The stack of the calling thread.
#[cfg(target_os = "macos")]
pub(crate) fn current_stack() -> Option<StackBounds> {
    // SAFETY: both calls read the calling thread's own attributes.
    let (high, size) = unsafe {
        let thread = libc::pthread_self();
        (
            libc::pthread_get_stackaddr_np(thread) as usize,
            libc::pthread_get_stacksize_np(thread),
        )
    };
    Some(StackBounds {
        low: high.checked_sub(size)?,
        high,
    })
}

/// The stack of the calling thread.
#[cfg(target_os = "linux")]
pub(crate) fn current_stack() -> Option<StackBounds> {
    // SAFETY: `attributes` is initialized by `pthread_getattr_np` before it
    // is read, and destroyed once after.
    unsafe {
        let mut attributes: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attributes) != 0 {
            return None;
        }
        let mut low: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let read = libc::pthread_attr_getstack(&attributes, &mut low, &mut size);
        libc::pthread_attr_destroy(&mut attributes);
        if read != 0 {
            return None;
        }
        Some(StackBounds {
            low: low as usize,
            high: (low as usize).checked_add(size)?,
        })
    }
}

/// No stack bounds on a target the sampler does not support.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn current_stack() -> Option<StackBounds> {
    None
}

/// The symbol the loader says `address` falls in, as `<symbol>+<offset>`.
/// Not for a signal handler: the name is copied into a new string.
pub(crate) fn symbol_at(address: usize) -> Option<String> {
    // SAFETY: an all-zero `Dl_info` is a valid value for `dladdr` to fill.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    // SAFETY: `dladdr` only reads the address and writes `info`.
    let found = unsafe { libc::dladdr(address as *const libc::c_void, &mut info) };
    if found == 0 || info.dli_sname.is_null() {
        return None;
    }
    // SAFETY: a non-null `dli_sname` is a NUL-terminated name the loader
    // keeps for the life of the image.
    let name = unsafe { std::ffi::CStr::from_ptr(info.dli_sname) };
    let offset = address.checked_sub(info.dli_saddr as usize)?;
    Some(format!("{}+{offset}", name.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_calling_threads_stack_holds_a_local() {
        let local = 0u64;
        let stack = current_stack();
        let address = std::ptr::addr_of!(local) as usize;
        assert!(
            stack.is_some_and(|stack| stack.contains(address)),
            "{stack:x?} should hold {address:#x}"
        );
    }
}
