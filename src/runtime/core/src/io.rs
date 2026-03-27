//! I/O primitives for the Miri runtime.
//!
//! Provides standard output and error stream operations callable from
//! compiled Miri code via FFI.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Tracks whether stdout output needs a trailing newline at program exit.
///
/// Set to `true` by `miri_rt_print` (no newline) and `false` by `miri_rt_println`.
/// Checked by an atexit handler to emit a final newline, preventing the shell
/// (e.g. zsh) from appending a `%` marker to incomplete output lines.
pub(super) static STDOUT_NEEDS_NEWLINE: AtomicBool = AtomicBool::new(false);
pub(super) static ATEXIT_REGISTERED: AtomicBool = AtomicBool::new(false);

extern "C" fn flush_trailing_newline() {
    if STDOUT_NEEDS_NEWLINE.load(Ordering::Relaxed) {
        let _ = io::stdout().write_all(b"\n");
        let _ = io::stdout().flush();
    }
}

pub(super) fn ensure_atexit_registered() {
    if !ATEXIT_REGISTERED.swap(true, Ordering::Relaxed) {
        unsafe {
            libc_atexit(flush_trailing_newline);
        }
    }
}

extern "C" {
    #[link_name = "atexit"]
    fn libc_atexit(func: extern "C" fn()) -> i32;
}

// Re-export FFI functions at module level for backward-compatible access
// via `miri_runtime_core::io::miri_rt_print` etc.
pub use ffi::*;

/// Stable FFI interface for I/O operations.
pub mod ffi {
    use super::*;
    use crate::string::MiriString;

    /// Prints a `MiriString` to stdout without a trailing newline.
    ///
    /// Flushes stdout immediately to ensure output appears before any subsequent
    /// operations. No-op if `s` is null.
    ///
    /// # Safety
    /// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_print(s: *const MiriString) {
        if s.is_null() {
            return;
        }
        ensure_atexit_registered();
        let text = (*s).as_str();
        print!("{}", text);
        let _ = std::io::stdout().flush();
        STDOUT_NEEDS_NEWLINE.store(!text.ends_with('\n'), Ordering::Relaxed);
    }

    /// Prints a `MiriString` to stdout with a trailing newline.
    ///
    /// Prints just a newline if `s` is null.
    ///
    /// # Safety
    /// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_println(s: *const MiriString) {
        if s.is_null() {
            println!();
            return;
        }
        println!("{}", (*s).as_str());
        STDOUT_NEEDS_NEWLINE.store(false, Ordering::Relaxed);
    }

    /// Prints a `MiriString` to stderr without a trailing newline.
    ///
    /// Flushes stderr immediately. No-op if `s` is null.
    ///
    /// # Safety
    /// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_eprint(s: *const MiriString) {
        if s.is_null() {
            return;
        }
        eprint!("{}", (*s).as_str());
        let _ = std::io::stderr().flush();
    }

    /// Prints a `MiriString` to stderr with a trailing newline.
    ///
    /// Prints just a newline if `s` is null.
    ///
    /// # Safety
    /// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_eprintln(s: *const MiriString) {
        if s.is_null() {
            eprintln!();
            return;
        }
        eprintln!("{}", (*s).as_str());
    }

    /// Returns the platform-specific line ending as a new `MiriString`.
    ///
    /// - Windows: `"\r\n"`
    /// - Unix/macOS: `"\n"`
    #[no_mangle]
    pub extern "C" fn miri_rt_get_line_end() -> *mut MiriString {
        #[cfg(windows)]
        const LINE_END: &str = "\r\n";

        #[cfg(not(windows))]
        const LINE_END: &str = "\n";

        crate::string::into_raw_ptr(MiriString::from_str(LINE_END))
    }
}
