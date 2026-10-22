// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What the profiler asks of the operating system: a CPU-time timer that
//! raises `SIGPROF`, a handler for it, the registers of the thread a signal
//! interrupted, and the bounds of the main thread's stack.
//!
//! The timer counts the CPU time of the whole process, so a program that
//! waits uses no samples. Linux uses a `timer_create` timer on the process
//! CPU clock that signals the main thread alone (`SIGEV_THREAD_ID`), so no
//! other thread, a GPU driver's included, is interrupted; while the main
//! thread waits on one that computes, its samples show where it waits.
//! macOS has neither per-process POSIX timers nor thread-directed delivery
//! and uses `setitimer(ITIMER_PROF)`, whose signal lands on whichever thread
//! the kernel picks: a sample on any thread but the main one records its
//! instruction alone, and a system call it interrupts is restarted
//! (`SA_RESTART`). Either way the signals add work to a profiled run, which a
//! GPU program's timing can show.

use super::frame_walk::{Registers, StackBounds};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};

/// Whether this target is one whose interrupted registers the sampler can
/// read.
pub(crate) const IS_SUPPORTED: bool = cfg!(all(
    any(target_os = "macos", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
));

/// The registers saved in the signal context `context`.
///
/// # Safety
/// `context` must be the third argument the kernel passed a handler
/// installed with `SA_SIGINFO`, or null.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe fn registers_of(context: *const libc::ucontext_t) -> Option<Registers> {
    // SAFETY: the caller hands over the kernel's context or null; on macOS
    // its machine context is a pointer, null when the kernel saved none.
    let state = unsafe { context.as_ref()?.uc_mcontext.as_ref()?.__ss };
    Some(Registers {
        pc: state.__pc as usize,
        fp: state.__fp as usize,
        sp: state.__sp as usize,
        lr: state.__lr as usize,
    })
}

/// The registers saved in the signal context `context`.
///
/// # Safety
/// As for the other targets' `registers_of`.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
unsafe fn registers_of(context: *const libc::ucontext_t) -> Option<Registers> {
    // SAFETY: as above; the machine context is a pointer, possibly null.
    let state = unsafe { context.as_ref()?.uc_mcontext.as_ref()?.__ss };
    Some(Registers {
        pc: state.__rip as usize,
        fp: state.__rbp as usize,
        sp: state.__rsp as usize,
        lr: 0,
    })
}

/// The registers saved in the signal context `context`.
///
/// # Safety
/// As for the other targets' `registers_of`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn registers_of(context: *const libc::ucontext_t) -> Option<Registers> {
    // SAFETY: the caller hands over the kernel's context or null; on Linux
    // the machine context is held inline.
    let registers = unsafe { &context.as_ref()?.uc_mcontext.gregs };
    let register = |index: libc::c_int| registers[index as usize] as usize;
    Some(Registers {
        pc: register(libc::REG_RIP),
        fp: register(libc::REG_RBP),
        sp: register(libc::REG_RSP),
        lr: 0,
    })
}

/// The registers saved in the signal context `context`.
///
/// # Safety
/// As for the other targets' `registers_of`.
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
unsafe fn registers_of(context: *const libc::ucontext_t) -> Option<Registers> {
    // SAFETY: as above; x29 is the frame pointer and x30 the link register.
    let machine = unsafe { &context.as_ref()?.uc_mcontext };
    Some(Registers {
        pc: machine.pc as usize,
        fp: machine.regs[29] as usize,
        sp: machine.sp as usize,
        lr: machine.regs[30] as usize,
    })
}

/// No registers: the sampler does not run on this target.
///
/// # Safety
/// Always safe; reads nothing.
#[cfg(not(all(
    any(target_os = "macos", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
)))]
unsafe fn registers_of(_context: *const libc::ucontext_t) -> Option<Registers> {
    None
}

/// The `SIGPROF` handler: reads the interrupted registers and records a
/// sample from them. Allocates nothing and takes no lock.
extern "C" fn on_profile_signal(
    _signal: libc::c_int,
    _info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    // SAFETY: the kernel passes the interrupted context as the third
    // argument of a handler installed with `SA_SIGINFO`.
    let registers = unsafe { registers_of(context as *const libc::ucontext_t) };
    if let Some(registers) = registers {
        super::record_sample(registers);
    }
}

/// Install the profiling handler for `SIGPROF`, restarting any system call a
/// sample interrupts. Returns whether the kernel accepted it.
pub(crate) fn install_handler() -> bool {
    let entry: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
        on_profile_signal;
    // SAFETY: an all-zero `sigaction` is a valid value of the C struct, and
    // every field the kernel reads is set below before it is passed.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = entry as usize;
    action.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;
    // SAFETY: `sa_mask` is a valid, owned signal set; `sigaction` reads the
    // action and writes no old action, as the null pointer asks.
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGPROF, &action, std::ptr::null_mut()) == 0
    }
}

/// Have `SIGPROF` ignored from here on, so a sample the stopped timer had
/// already raised cannot reach the default action, which ends the process.
pub(crate) fn ignore_signal() {
    // SAFETY: setting a signal's disposition to `SIG_IGN` touches no memory
    // of this process.
    unsafe {
        libc::signal(libc::SIGPROF, libc::SIG_IGN);
    }
}

/// The interval between samples at `hz` per second, as a timer takes it.
fn period_of(hz: u32) -> (libc::time_t, i64) {
    let nanoseconds = 1_000_000_000 / u64::from(hz.max(1));
    (
        (nanoseconds / 1_000_000_000) as libc::time_t,
        (nanoseconds % 1_000_000_000) as i64,
    )
}

/// Start sampling `hz` times per second of process CPU time. Returns whether
/// the timer was armed.
#[cfg(target_os = "macos")]
pub(crate) fn start_timer(hz: u32) -> bool {
    let (seconds, nanoseconds) = period_of(hz);
    let period = libc::timeval {
        tv_sec: seconds,
        tv_usec: (nanoseconds / 1000).max(1) as libc::suseconds_t,
    };
    let timer = libc::itimerval {
        it_interval: period,
        it_value: period,
    };
    // SAFETY: `timer` is a valid `itimerval`; no old value is asked for.
    unsafe { libc::setitimer(libc::ITIMER_PROF, &timer, std::ptr::null_mut()) == 0 }
}

/// Stop the timer `start_timer` armed.
#[cfg(target_os = "macos")]
pub(crate) fn stop_timer() {
    // SAFETY: an all-zero `itimerval` disarms the timer.
    unsafe {
        let disarmed: libc::itimerval = std::mem::zeroed();
        libc::setitimer(libc::ITIMER_PROF, &disarmed, std::ptr::null_mut());
    }
}

/// The POSIX timer `start_timer` created, or 0.
#[cfg(target_os = "linux")]
static TIMER: AtomicUsize = AtomicUsize::new(0);

/// Start sampling `hz` times per second of process CPU time, the signal
/// directed at the calling thread. Returns whether the timer was armed.
#[cfg(target_os = "linux")]
pub(crate) fn start_timer(hz: u32) -> bool {
    // SAFETY: an all-zero `sigevent` is a valid value; the fields the kernel
    // reads for `SIGEV_THREAD_ID` are set below.
    let mut event: libc::sigevent = unsafe { std::mem::zeroed() };
    event.sigev_notify = libc::SIGEV_THREAD_ID;
    event.sigev_signo = libc::SIGPROF;
    // SAFETY: `gettid` has no preconditions. Called on the main thread, at
    // start-up, so the signal goes to the thread whose stack a sample walks.
    event.sigev_notify_thread_id = unsafe { libc::gettid() };
    let mut timer: libc::timer_t = std::ptr::null_mut();
    // SAFETY: `event` and `timer` are valid for the call to read and write.
    let created =
        unsafe { libc::timer_create(libc::CLOCK_PROCESS_CPUTIME_ID, &mut event, &mut timer) };
    if created != 0 {
        return false;
    }
    TIMER.store(timer as usize, Ordering::Release);
    let (seconds, nanoseconds) = period_of(hz);
    let period = libc::timespec {
        tv_sec: seconds,
        tv_nsec: nanoseconds as _,
    };
    let schedule = libc::itimerspec {
        it_interval: period,
        it_value: period,
    };
    // SAFETY: `timer` was just created and `schedule` is a valid value.
    unsafe { libc::timer_settime(timer, 0, &schedule, std::ptr::null_mut()) == 0 }
}

/// Stop the timer `start_timer` armed.
#[cfg(target_os = "linux")]
pub(crate) fn stop_timer() {
    let timer = TIMER.swap(0, Ordering::AcqRel);
    if timer != 0 {
        // SAFETY: `timer` is the timer `start_timer` created, deleted once
        // because the swap above hands it out once.
        unsafe {
            libc::timer_delete(timer as libc::timer_t);
        }
    }
}

/// No timer on a target the sampler does not support.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn start_timer(_hz: u32) -> bool {
    false
}

/// No timer on a target the sampler does not support.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn stop_timer() {}

/// The calling thread, as a number a handler compares against.
pub(crate) fn current_thread() -> usize {
    // SAFETY: `pthread_self` has no preconditions.
    unsafe { libc::pthread_self() as usize }
}

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

    #[test]
    fn a_period_is_one_second_divided_by_the_rate() {
        assert_eq!(period_of(1000), (0, 1_000_000));
        assert_eq!(period_of(1), (1, 0));
        assert_eq!(period_of(0), (1, 0));
    }

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
