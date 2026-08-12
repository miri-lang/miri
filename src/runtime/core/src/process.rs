// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Process control for the Miri runtime.
//!
//! Provides `exit(code int)` to terminate the process with an exit code.

pub mod ffi {
    /// Terminates the process with the given exit code.
    ///
    /// Flushes stdout and stderr before exiting to ensure buffered output is visible.
    /// Uses `libc::_exit` instead of `libc::exit` to skip atexit handlers (which would
    /// otherwise run the leak checker and override the exit status).
    ///
    /// Only the low 8 bits of the exit code are visible to the OS.
    ///
    /// # Safety
    /// Always safe; does not return.
    #[no_mangle]
    pub extern "C" fn miri_rt_exit(code: i64) -> ! {
        // Flush stdout and stderr
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();

        // Exit without running atexit handlers
        // SAFETY: This function never returns, so the divergence is guaranteed.
        unsafe { libc::_exit((code & 0xFF) as i32) }
    }
}
