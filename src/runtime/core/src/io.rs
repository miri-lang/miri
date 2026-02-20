//! I/O primitives for Miri runtime.
//!
//! Provides basic input/output operations.

use crate::string::MiriString;
use std::io::{self, Write};

/// Prints a MiriString to stdout without a trailing newline.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString`.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_print(s: *const MiriString) {
    if s.is_null() {
        return;
    }

    let str_val = (*s).as_str();
    print!("{}", str_val);

    // Flush to ensure immediate output
    let _ = io::stdout().flush();
}

/// Prints a MiriString to stdout with a trailing newline.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString`.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_println(s: *const MiriString) {
    if s.is_null() {
        println!();
        return;
    }

    let str_val = (*s).as_str();
    println!("{}", str_val);
}

/// Prints a MiriString to stderr without a trailing newline.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString`.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_eprint(s: *const MiriString) {
    if s.is_null() {
        return;
    }

    let str_val = (*s).as_str();
    eprint!("{}", str_val);

    let _ = io::stderr().flush();
}

/// Prints a MiriString to stderr with a trailing newline.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString`.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_eprintln(s: *const MiriString) {
    if s.is_null() {
        eprintln!();
        return;
    }

    let str_val = (*s).as_str();
    eprintln!("{}", str_val);
}

/// Returns the platform-specific line ending as a new MiriString.
///
/// - Windows: "\r\n"
/// - Unix/macOS: "\n"
#[no_mangle]
pub unsafe extern "C" fn miri_rt_get_line_end() -> *mut MiriString {
    #[cfg(windows)]
    let line_end = "\r\n";

    #[cfg(not(windows))]
    let line_end = "\n";

    let string = Box::new(MiriString::from_str(line_end));
    Box::into_raw(string)
}
