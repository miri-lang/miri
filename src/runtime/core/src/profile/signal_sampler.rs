// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The Linux sampler: a `timer_create` timer on the process CPU clock that
//! raises `SIGPROF` on the main thread alone (`SIGEV_THREAD_ID`), and a
//! handler that reads the interrupted registers from the signal context and
//! records a sample from them. No other thread, a GPU driver's included, is
//! interrupted; while the main thread waits on one that computes, its samples
//! show where it waits. The signals add work to a profiled run, which a GPU
//! program's timing can show.

use super::frame_walk::Registers;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The thread `start` ran on, the program's main thread, whose stack a
/// sample walks.
static MAIN_THREAD: AtomicUsize = AtomicUsize::new(0);

/// The calling thread, as a number the handler compares against.
fn current_thread() -> usize {
    // SAFETY: `pthread_self` has no preconditions.
    unsafe { libc::pthread_self() as usize }
}

/// The registers saved in the signal context `context`.
///
/// # Safety
/// `context` must be the third argument the kernel passed a handler
/// installed with `SA_SIGINFO`, or null.
#[cfg(target_arch = "x86_64")]
unsafe fn registers_of(context: *const libc::ucontext_t) -> Option<Registers> {
    // SAFETY: the caller hands over the kernel's context or null; the
    // machine context is held inline.
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
#[cfg(target_arch = "aarch64")]
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

/// No registers: the sampler does not run on this architecture.
///
/// # Safety
/// Always safe; reads nothing.
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
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
        super::record_sample(
            registers,
            current_thread() == MAIN_THREAD.load(Ordering::Relaxed),
        );
    }
}

/// Install the profiling handler for `SIGPROF`, restarting any system call a
/// sample interrupts. Returns whether the kernel accepted it.
fn install_handler() -> bool {
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
fn ignore_signal() {
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

/// The POSIX timer `start_timer` created, or 0.
static TIMER: AtomicUsize = AtomicUsize::new(0);

/// Start sampling `hz` times per second of process CPU time, the signal
/// directed at the calling thread. Returns whether the timer was armed.
fn start_timer(hz: u32) -> bool {
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
fn stop_timer() {
    let timer = TIMER.swap(0, Ordering::AcqRel);
    if timer != 0 {
        // SAFETY: `timer` is the timer `start_timer` created, deleted once
        // because the swap above hands it out once.
        unsafe {
            libc::timer_delete(timer as libc::timer_t);
        }
    }
}

/// Start sampling the calling thread, the program's main thread, `hz` times
/// per second of process CPU time. `Err` says what could not be set up.
pub(crate) fn start(hz: u32) -> Result<(), &'static str> {
    MAIN_THREAD.store(current_thread(), Ordering::Relaxed);
    if !install_handler() {
        return Err("the profiling signal handler could not be installed");
    }
    if !start_timer(hz) {
        return Err("the profiling timer could not be started");
    }
    Ok(())
}

/// Stop the timer, and have any signal it already raised ignored.
pub(crate) fn stop() {
    stop_timer();
    ignore_signal();
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
}
