// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reports a program's call stack running out as a runtime error, not a signal.
//!
//! A frame that outgrows the stack faults on the guard region below it. The
//! kernel can only deliver that fault on a stack that still has room, so the
//! handler runs on an alternate stack set up at load. A fault within reach of
//! the stack pointer is the stack running out and ends the program through the
//! trap path; any other fault goes back to the handler that was there before,
//! and the program ends by its signal as it would have.
//!
//! Only the process's main thread runs compiled Miri code and only it gets the
//! alternate stack. Its stack size is the process's own limit.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Room for the handler itself, and for what the trap report touches.
const ALT_STACK_BYTES: usize = 64 * 1024;

/// How far from the stack pointer a fault can land on a frame being set up.
/// A frame past one page is touched a page at a time as it is allocated, so a
/// fault on any frame lies within this reach.
const FAULT_REACH: usize = 64 * 1024;

/// The bytes of the alternate stack.
#[repr(align(16))]
struct AltStack(UnsafeCell<[u8; ALT_STACK_BYTES]>);

// SAFETY: the kernel is the only reader; this runtime passes its address once.
unsafe impl Sync for AltStack {}

static ALT_STACK: AltStack = AltStack(UnsafeCell::new([0; ALT_STACK_BYTES]));

/// The handlers displaced for each signal, restored when a fault is not an
/// overflow.
struct SavedHandlers(UnsafeCell<[libc::sigaction; 2]>);

// SAFETY: written once at load before any fault, and read only by the handler.
unsafe impl Sync for SavedHandlers {}

// SAFETY: a zeroed `sigaction` is a plain value naming the default action.
static SAVED_HANDLERS: SavedHandlers =
    SavedHandlers(UnsafeCell::new(unsafe { std::mem::zeroed() }));

/// The signals a guard-page fault arrives as, in [`SAVED_HANDLERS`] order.
const FAULT_SIGNALS: [libc::c_int; 2] = [libc::SIGSEGV, libc::SIGBUS];

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether a fault at `fault` with the stack pointer at `sp` is the stack
/// running out: only a frame being set up writes that close to the stack
/// pointer, and a live stack is mapped there.
fn is_stack_fault(fault: usize, sp: usize) -> bool {
    fault.abs_diff(sp) <= FAULT_REACH
}

/// The interrupted stack pointer in the signal context `context`.
///
/// # Safety
/// `context` must be the third argument the kernel passed a handler installed
/// with `SA_SIGINFO`.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe fn stack_pointer(context: *const libc::ucontext_t) -> Option<usize> {
    // SAFETY: the kernel's context holds its machine state behind this pointer.
    let machine = unsafe { (*context).uc_mcontext.as_ref()? };
    Some(machine.__ss.__sp as usize)
}

/// As for the other targets' `stack_pointer`.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
unsafe fn stack_pointer(context: *const libc::ucontext_t) -> Option<usize> {
    // SAFETY: as above.
    let machine = unsafe { (*context).uc_mcontext.as_ref()? };
    Some(machine.__ss.__rsp as usize)
}

/// As for the other targets' `stack_pointer`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn stack_pointer(context: *const libc::ucontext_t) -> Option<usize> {
    // SAFETY: the machine context is held inline.
    let registers = unsafe { &(*context).uc_mcontext.gregs };
    Some(registers[libc::REG_RSP as usize] as usize)
}

/// As for the other targets' `stack_pointer`.
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
unsafe fn stack_pointer(context: *const libc::ucontext_t) -> Option<usize> {
    // SAFETY: as above.
    Some(unsafe { (*context).uc_mcontext.sp } as usize)
}

/// No stack pointer on other targets: every fault keeps its signal.
#[cfg(not(any(
    all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
)))]
unsafe fn stack_pointer(_context: *const libc::ucontext_t) -> Option<usize> {
    None
}

/// The handler for a guard-page fault's two signals.
///
/// # Safety
/// Called only by the kernel, as a handler installed with `SA_SIGINFO`.
unsafe extern "C" fn on_fault(
    signal: libc::c_int,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    // SAFETY: the kernel passes a valid info record and context to a handler.
    let fault = unsafe { (*info).si_addr() } as usize;
    let sp = unsafe { stack_pointer(context.cast()) };
    if sp.is_some_and(|sp| is_stack_fault(fault, sp)) {
        crate::trap::trap_stack_overflow();
    }
    // Not an overflow. Put back what was here before; returning re-runs the
    // faulting instruction, which ends the program by its signal as before.
    let index = FAULT_SIGNALS.iter().position(|s| *s == signal);
    if let Some(index) = index {
        // SAFETY: the saved handlers were written at load, and a previous
        // handler is a valid `sigaction`.
        unsafe {
            let saved = &(*SAVED_HANDLERS.0.get())[index];
            libc::sigaction(signal, saved, std::ptr::null_mut());
        }
    }
}

/// Installs the handler on the calling thread's alternate stack. Called once
/// at load, on the main thread, before compiled code runs.
///
/// # Safety
/// Must be called only from the load-time initialiser, on the main thread.
pub(crate) unsafe fn install() {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    let stack = libc::stack_t {
        ss_sp: ALT_STACK.0.get().cast(),
        ss_flags: 0,
        ss_size: ALT_STACK_BYTES,
    };
    // SAFETY: the alternate stack is static and lives for the whole process.
    if unsafe { libc::sigaltstack(&stack, std::ptr::null_mut()) } != 0 {
        return;
    }
    // SAFETY: a zeroed `sigaction` is valid, and the fields set below make it
    // the handler this module defines.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = on_fault as *const () as usize;
    action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
    // SAFETY: the mask is the zeroed value, and each saved slot is written
    // before any fault reaches the handler.
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        let saved = &mut *SAVED_HANDLERS.0.get();
        for (slot, signal) in saved.iter_mut().zip(FAULT_SIGNALS) {
            libc::sigaction(signal, &action, slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_stack_fault, FAULT_REACH};

    #[test]
    fn a_fault_beside_the_stack_pointer_is_the_stack_running_out() {
        let sp = 0x7fff_0000_0000;
        assert!(is_stack_fault(sp - 8, sp));
        assert!(is_stack_fault(sp + 8, sp));
        assert!(is_stack_fault(sp - FAULT_REACH, sp));
    }

    #[test]
    fn a_fault_away_from_the_stack_pointer_is_not() {
        let sp = 0x7fff_0000_0000;
        assert!(!is_stack_fault(8, sp));
        assert!(!is_stack_fault(sp - FAULT_REACH - 1, sp));
        assert!(!is_stack_fault(sp + FAULT_REACH + 1, sp));
    }
}
