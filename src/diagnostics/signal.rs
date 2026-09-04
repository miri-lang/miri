// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Signal number to diagnostic code and description mapping.
//!
//! This module provides platform-independent translation of UNIX signal numbers
//! to diagnostic codes and human-readable signal names. It is used by both the
//! run path in `src/main.rs` and the test runner in `src/test_runner/runner.rs`.

use crate::diagnostics::codes::DiagnosticCode;

#[cfg(unix)]
use libc;

/// Return the diagnostic code and signal name for a given signal number.
/// On non-UNIX platforms, this function must not be called.
#[cfg(unix)]
pub fn signal_to_code_and_name(signal: i32) -> (DiagnosticCode, &'static str) {
    match signal {
        libc::SIGSEGV => (DiagnosticCode::RtSegmentationFault, "SIGSEGV"),
        libc::SIGBUS => (DiagnosticCode::RtBusError, "SIGBUS"),
        libc::SIGILL => (DiagnosticCode::RtIllegalInstruction, "SIGILL"),
        libc::SIGABRT => (DiagnosticCode::RtAbort, "SIGABRT"),
        libc::SIGKILL => (DiagnosticCode::RtSignalTerminated, "SIGKILL"),
        libc::SIGTERM => (DiagnosticCode::RtSignalTerminated, "SIGTERM"),
        libc::SIGFPE => (DiagnosticCode::RtSignalTerminated, "SIGFPE"),
        libc::SIGPIPE => (DiagnosticCode::RtSignalTerminated, "SIGPIPE"),
        #[cfg(not(target_os = "windows"))]
        libc::SIGHUP => (DiagnosticCode::RtSignalTerminated, "SIGHUP"),
        #[cfg(not(target_os = "windows"))]
        libc::SIGINT => (DiagnosticCode::RtSignalTerminated, "SIGINT"),
        #[cfg(not(target_os = "windows"))]
        libc::SIGQUIT => (DiagnosticCode::RtSignalTerminated, "SIGQUIT"),
        #[cfg(not(target_os = "windows"))]
        libc::SIGTRAP => (DiagnosticCode::RtSignalTerminated, "SIGTRAP"),
        #[cfg(not(target_os = "windows"))]
        libc::SIGSYS => (DiagnosticCode::RtSignalTerminated, "SIGSYS"),
        _ => (DiagnosticCode::RtSignalTerminated, "SIGNAL"),
    }
}

/// Return the human-readable message for a signal-terminated process.
/// The message follows the format: `terminated by signal N (SIGNAME)`
#[cfg(unix)]
pub fn signal_message(signal: i32) -> String {
    let (_, name) = signal_to_code_and_name(signal);
    format!("terminated by signal {} ({})", signal, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_signal_to_code_and_name_segv() {
        let (code, name) = signal_to_code_and_name(libc::SIGSEGV);
        assert_eq!(code, DiagnosticCode::RtSegmentationFault);
        assert_eq!(name, "SIGSEGV");
        assert_eq!(
            signal_message(libc::SIGSEGV),
            format!("terminated by signal {} (SIGSEGV)", libc::SIGSEGV)
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_signal_to_code_and_name_sigbus() {
        let (code, name) = signal_to_code_and_name(libc::SIGBUS);
        assert_eq!(code, DiagnosticCode::RtBusError);
        assert_eq!(name, "SIGBUS");
    }

    #[test]
    #[cfg(unix)]
    fn test_signal_to_code_and_name_sigill() {
        let (code, name) = signal_to_code_and_name(libc::SIGILL);
        assert_eq!(code, DiagnosticCode::RtIllegalInstruction);
        assert_eq!(name, "SIGILL");
    }

    #[test]
    #[cfg(unix)]
    fn test_signal_to_code_and_name_sigabrt() {
        let (code, name) = signal_to_code_and_name(libc::SIGABRT);
        assert_eq!(code, DiagnosticCode::RtAbort);
        assert_eq!(name, "SIGABRT");
    }

    #[test]
    #[cfg(unix)]
    fn test_signal_to_code_and_name_catchall() {
        let (code, name) = signal_to_code_and_name(9999);
        assert_eq!(code, DiagnosticCode::RtSignalTerminated);
        assert_eq!(name, "SIGNAL");
        assert_eq!(signal_message(9999), "terminated by signal 9999 (SIGNAL)");
    }
}
