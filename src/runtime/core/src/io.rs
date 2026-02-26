//! I/O primitives for the Miri runtime.
//!
//! Provides standard output and error stream operations callable from
//! compiled Miri code via FFI.

use crate::string::MiriString;
use std::io::{self, Write};

/// Prints a `MiriString` to stdout without a trailing newline.
///
/// Flushes stdout immediately to ensure output appears before any subsequent
/// operations. No-op if `s` is null.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_print(s: *const MiriString) {
    if s.is_null() {
        return;
    }
    print!("{}", (*s).as_str());
    let _ = io::stdout().flush();
}

/// Prints a `MiriString` to stdout with a trailing newline.
///
/// Prints just a newline if `s` is null.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_println(s: *const MiriString) {
    if s.is_null() {
        println!();
        return;
    }
    println!("{}", (*s).as_str());
}

/// Prints a `MiriString` to stderr without a trailing newline.
///
/// Flushes stderr immediately. No-op if `s` is null.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_eprint(s: *const MiriString) {
    if s.is_null() {
        return;
    }
    eprint!("{}", (*s).as_str());
    let _ = io::stderr().flush();
}

/// Prints a `MiriString` to stderr with a trailing newline.
///
/// Prints just a newline if `s` is null.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
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
