// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The one way a Miri program dies of a runtime fault.
//!
//! A trap has two audiences. A person reads the sentence on stderr; a tool
//! reads the diagnostic code out of the envelope `miri run` prints, and decides
//! from it whether the program ran. Both are produced here, by the single
//! function that ends the process, so a trap cannot report one and forget the
//! other — which is exactly what an out-of-bounds index and an explicit
//! `panic` used to do, dying with a sentence and no code, and leaving a tool
//! reading a crash as a successful run.

use crate::abi_gpu_fault::{recorded_faults, write_summary};

/// The codes this runtime can raise.
///
/// The runtime is its own crate and cannot reach the compiler's registry, so
/// the codes travel as text. A test on the compiler side reads these back and
/// fails if one of them is not a registered runtime code.
pub(crate) mod code {
    /// An integer division whose divisor was zero.
    pub(crate) const DIVISION_BY_ZERO: &str = "MER_RT_001";
    /// An integer remainder whose divisor was zero.
    pub(crate) const REMAINDER_BY_ZERO: &str = "MER_RT_002";
    /// An integer `+`, `-`, `*` or negation whose result does not fit its type.
    pub(crate) const INTEGER_OVERFLOW: &str = "MER_RT_003";
    /// An argument that does not satisfy its parameter's guard.
    pub(crate) const INVALID_OPERAND: &str = "MER_RT_004";
    /// An assertion that did not hold.
    pub(crate) const ASSERTION_FAILED: &str = "MER_RT_005";
    /// An index outside the collection it was applied to.
    pub(crate) const INDEX_OUT_OF_BOUNDS: &str = "MER_RT_011";
    /// A `panic` the program asked for.
    pub(crate) const EXPLICIT_PANIC: &str = "MER_RT_012";
    /// A GPU kernel launch the runtime or the device refused.
    pub(crate) const GPU_LAUNCH_FAILED: &str = "MER_RT_013";
    /// A method reached at an instance of its class the compiler withheld it
    /// at, because its obligations were not checked there.
    pub(crate) const METHOD_NOT_CHECKED: &str = "MER_RT_014";
    /// A heap block released twice, written after its release, or released
    /// at a size larger than it was allocated at.
    pub(crate) const HEAP_CORRUPTED: &str = "MER_RT_015";
    /// A list changed by the code the runtime was sorting it with: a closure,
    /// or an element's own `compare`.
    pub(crate) const LIST_CHANGED_WHILE_SORTED: &str = "MER_RT_016";
    /// A writing method handed a receiver something else still holds.
    pub(crate) const RECEIVER_SHARED: &str = "MER_RT_017";
    /// A heap block handed to the runtime after it was released, caught by
    /// the heap guard.
    pub(crate) const USE_AFTER_FREE: &str = "MER_RT_018";
    /// A value a range was asked to step from that has no next value: a
    /// bound whose `steppable()` is false, or a string that is not exactly
    /// one character, or is the last code point, asked for its successor.
    pub(crate) const NOT_STEPPABLE: &str = "MER_RT_019";
    /// A program's calls nested deeper than its stack.
    pub(crate) const STACK_OVERFLOW: &str = "MER_RT_020";
}

/// The sentences a trap reports for the faults the host and a GPU kernel
/// both stop the program for, read from the GPU fault table so the two say
/// the same thing.
pub(crate) mod message {
    use crate::abi_gpu_fault::GpuFault;

    /// An integer division whose divisor was zero.
    pub(crate) const DIVISION_BY_ZERO: &str = GpuFault::DivisionByZero.sentence();
    /// An integer remainder whose divisor was zero.
    pub(crate) const REMAINDER_BY_ZERO: &str = GpuFault::RemainderByZero.sentence();
    /// An integer result that does not fit its type.
    pub(crate) const INTEGER_OVERFLOW: &str = GpuFault::IntegerOverflow.sentence();
}

/// The report of every kind of fault the GPU fault word `word` records, each
/// with its code, and the code the trap leaves for a tool: that of the lowest
/// kind recorded. `None` when `word` records no kind this runtime knows.
fn gpu_fault_summary(word: u32) -> Option<(&'static str, String)> {
    let first = recorded_faults(word).next()?;
    let mut summary = String::new();
    write_summary(word, &mut summary).ok()?;
    Some((first.code(), summary))
}

/// Reports every kind of fault the GPU fault word `word` records, in one
/// error, and ends the process.
pub(crate) fn trap_gpu_faults(word: u32) -> ! {
    match gpu_fault_summary(word) {
        Some((first_code, summary)) => trap(first_code, &summary),
        None => trap(
            code::GPU_LAUNCH_FAILED,
            &format!("a GPU kernel recorded a fault this runtime does not know: {word:#x}"),
        ),
    }
}

/// Report a runtime fault under `code` and end the process.
///
/// `_exit` rather than `abort`: SIGABRT brings up `ReportCrash` on macOS and
/// serializes a parallel test run behind the crash daemon, and skipping the
/// atexit handlers keeps the leak observer from reporting an allocation the
/// trap simply never got to free.
pub(crate) fn trap(code: &str, message: &str) -> ! {
    use std::io::Write;

    eprintln!("Runtime error: {}", message);
    write_trap_report(code);
    // A profiled run keeps the samples it took up to the trap. Writing them
    // allocates, which a heap just found corrupted or touched after release
    // cannot be trusted with, so those traps leave no record.
    if code != code::HEAP_CORRUPTED && code != code::USE_AFTER_FREE {
        crate::profile::finish();
    }
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(1) }
}

/// Report the stack running out and end the process.
///
/// A signal handler on the alternate stack runs this, where the interrupted
/// frame may hold the allocator's lock: it only writes to file descriptors and
/// allocates nothing, unlike [`trap`], so it leaves no profile record either.
pub(crate) fn trap_stack_overflow() -> ! {
    write_raw(
        2,
        b"Runtime error: stack overflow: the program's calls are nested deeper than its stack\n",
    );
    write_trap_report_raw(code::STACK_OVERFLOW);
    unsafe { libc::_exit(1) }
}

/// Writes `bytes` to the descriptor `fd`, ignoring a failure: there is nothing
/// a dying program can do about one.
fn write_raw(fd: libc::c_int, bytes: &[u8]) {
    // SAFETY: the buffer is valid for its length for the whole call.
    let _ = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
}

/// [`write_trap_report`], without allocating.
fn write_trap_report_raw(code: &str) {
    // SAFETY: the name is a valid C string; the value is a live C string or null.
    let path = unsafe { libc::getenv(c"MIRI_TRAP_REPORT_PATH".as_ptr()) };
    if path.is_null() {
        return;
    }
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
    // SAFETY: `path` is the C string just checked, and the mode is the file's.
    let fd = unsafe { libc::open(path, flags, 0o600 as libc::c_uint) };
    if fd >= 0 {
        write_raw(fd, code.as_bytes());
        // SAFETY: `fd` was just opened and belongs to this call.
        unsafe { libc::close(fd) };
    }
}

/// Record the code of a trap where the compiler that spawned this program can
/// read it.
///
/// The path arrives in the environment as `MIRI_TRAP_REPORT_PATH`, and the
/// report is best-effort: nothing about the trap depends on the write landing,
/// and a run started without the variable simply leaves no report.
fn write_trap_report(code: &str) {
    if let Ok(path) = std::env::var("MIRI_TRAP_REPORT_PATH") {
        let _ = std::fs::write(&path, code);
    }
}

#[cfg(test)]
mod tests {
    use super::{code, gpu_fault_summary};
    use crate::abi_gpu_fault::GpuFault;

    #[test]
    fn a_gpu_fault_word_reports_every_kind_it_records_under_the_lowest_code() {
        let word = GpuFault::DivisionByZero.bit() | GpuFault::IndexOutOfBounds.bit();
        assert_eq!(
            gpu_fault_summary(word),
            Some((
                "MER_RT_001",
                "a GPU kernel faulted: division by zero (MER_RT_001); \
                 index out of bounds (MER_RT_011)"
                    .to_string()
            ))
        );
    }

    #[test]
    fn a_gpu_overflow_reports_the_hosts_overflow_sentence() {
        let summary = gpu_fault_summary(GpuFault::IntegerOverflow.bit());
        assert!(
            summary
                .as_ref()
                .is_some_and(|(code, sentence)| *code == "MER_RT_003"
                    && sentence.contains("integer overflow: the result does not fit its type")),
            "{summary:?}"
        );
    }

    /// The GPU fault table names each kind's code itself, so it is held to
    /// the code the host's own trap raises.
    #[test]
    fn every_gpu_fault_carries_the_code_of_the_hosts_trap() {
        for fault in GpuFault::ALL {
            let host = match fault {
                GpuFault::IntegerOverflow => code::INTEGER_OVERFLOW,
                GpuFault::DivisionByZero => code::DIVISION_BY_ZERO,
                GpuFault::RemainderByZero => code::REMAINDER_BY_ZERO,
                GpuFault::IndexOutOfBounds => code::INDEX_OUT_OF_BOUNDS,
            };
            assert_eq!(fault.code(), host, "{fault:?}");
        }
    }

    #[test]
    fn a_gpu_fault_word_with_no_known_kind_has_no_summary() {
        assert_eq!(gpu_fault_summary(0), None);
        assert_eq!(gpu_fault_summary(1 << 31), None);
    }
}
