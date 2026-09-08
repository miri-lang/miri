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
    /// An assertion that did not hold.
    pub(crate) const ASSERTION_FAILED: &str = "MER_RT_005";
    /// An index outside the collection it was applied to.
    pub(crate) const INDEX_OUT_OF_BOUNDS: &str = "MER_RT_011";
    /// A `panic` the program asked for.
    pub(crate) const EXPLICIT_PANIC: &str = "MER_RT_012";
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
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(1) }
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
