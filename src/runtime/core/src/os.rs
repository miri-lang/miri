// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Environment and platform information for the Miri runtime.
//!
//! Provides environment variable access and program arguments.
//!
//! # Status Channel
//!
//! `miri_rt_env_set` uses a thread-local status code to report errors, similar
//! to the filesystem module. Status codes:
//! - 0 = success
//! - 1 = invalid variable name (empty, contains `=` or NUL)
//! - 2 = invalid variable value (contains NUL)
//! - 3 = other OS error

use std::cell::RefCell;
use std::sync::OnceLock;

use crate::string::{into_raw_ptr, MiriString};

thread_local! {
    /// Thread-local status code for the last environment operation.
    /// 0 = success, 1-3 = error codes (see module doc).
    static ENV_STATUS: RefCell<i64> = const { RefCell::new(0) };
    /// Holds the OS error message for the last failure (populated only when status == 3).
    static ENV_ERROR_MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Sets the thread-local status code and clears the error message on success.
fn set_env_status(code: i64, msg: String) {
    ENV_STATUS.with(|s| {
        *s.borrow_mut() = code;
    });
    if code != 0 {
        ENV_ERROR_MESSAGE.with(|m| {
            *m.borrow_mut() = msg;
        });
    } else {
        ENV_ERROR_MESSAGE.with(|m| {
            m.borrow_mut().clear();
        });
    }
}

/// Snapshot of command-line arguments (excluding argv[0]).
/// Captured once and reused for consistent `length()` and `element_at()` calls.
static ARGS_SNAPSHOT: OnceLock<Vec<String>> = OnceLock::new();

/// The arguments the program was started with, excluding argv[0].
///
/// Captured on first use and reused afterwards, so `length()` and `element_at()`
/// cannot disagree part-way through a loop. An argument that is not valid UTF-8 is
/// converted lossily, because a Miri String is UTF-8.
fn args_snapshot() -> &'static Vec<String> {
    ARGS_SNAPSHOT.get_or_init(|| {
        std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    })
}

pub mod ffi {
    use super::*;

    /// Checks whether an environment variable is set.
    /// Returns 1 if set, 0 if not. Never fails (always status = 0).
    ///
    /// # Safety
    /// The caller must ensure that `name` is a valid pointer to a MiriString.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_env_has(name: *const MiriString) -> i64 {
        if name.is_null() {
            set_env_status(0, String::new());
            return 0;
        }

        let name_str = (*name).as_str();
        set_env_status(0, String::new());
        if std::env::var(name_str).is_ok() {
            1
        } else {
            0
        }
    }

    /// Retrieves the value of an environment variable.
    /// Returns a newly allocated MiriString on success, empty string on failure.
    ///
    /// Status codes:
    /// - 0 = success (variable was set)
    /// - 0 = success but variable is unset (returns empty string and status 0)
    ///   Note: Use `miri_rt_env_has` to distinguish "unset" from "set to empty".
    ///
    /// # Safety
    /// The caller must ensure that `name` is a valid pointer to a MiriString.
    /// The returned pointer must be DecRef'd by the caller when no longer needed.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_env_get(name: *const MiriString) -> *mut MiriString {
        if name.is_null() {
            set_env_status(0, String::new());
            return into_raw_ptr(MiriString::from_str(""));
        }

        let name_str = (*name).as_str();
        set_env_status(0, String::new());
        let value = std::env::var(name_str).unwrap_or_default();
        into_raw_ptr(MiriString::from_str(&value))
    }

    /// Sets an environment variable and returns whether an existing value was replaced.
    ///
    /// Status codes:
    /// - 0 = success (return value indicates if replaced)
    /// - 1 = invalid name (empty, contains `=` or NUL)
    /// - 2 = invalid value (contains NUL)
    /// - 3 = other OS error
    ///
    /// Returns i64: 1 if an existing value was replaced, 0 if newly created.
    ///
    /// # Safety
    /// The caller must ensure that both `name` and `value` are valid pointers to MiriStrings.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_env_set(
        name: *const MiriString,
        value: *const MiriString,
    ) -> i64 {
        if name.is_null() || value.is_null() {
            set_env_status(3, "null pointer".to_string());
            return 0;
        }

        let name_str = (*name).as_str();
        let value_str = (*value).as_str();

        // Validate name
        if name_str.is_empty() {
            set_env_status(1, String::new());
            return 0;
        }
        if name_str.contains('=') || name_str.contains('\0') {
            set_env_status(1, String::new());
            return 0;
        }

        // Validate value
        if value_str.contains('\0') {
            set_env_status(2, String::new());
            return 0;
        }

        // Check if the variable was already set
        let was_set = std::env::var(name_str).is_ok();

        // Set the environment variable (this only affects this process and children)
        std::env::set_var(name_str, value_str);

        set_env_status(0, String::new());
        if was_set {
            1
        } else {
            0
        }
    }

    /// Returns the number of command-line arguments (excluding argv[0]).
    ///
    /// # Safety
    /// Always safe; no memory access beyond the OnceLock.
    #[no_mangle]
    pub extern "C" fn miri_rt_args_count() -> i64 {
        args_snapshot().len() as i64
    }

    /// Returns the argument at the given 0-based index, which does not include
    /// argv[0]. An out-of-range index reports the bounds and terminates the
    /// process, the way array indexing does.
    ///
    /// Allocates and returns a MiriString with RC=1. Caller must DecRef it.
    ///
    /// # Safety
    /// Always safe; an out-of-range index exits rather than reading out of bounds.
    #[no_mangle]
    pub extern "C" fn miri_rt_args_at(index: i64) -> *mut MiriString {
        let args = args_snapshot();

        if index < 0 || index as usize >= args.len() {
            // `_exit` rather than `abort`, matching miri_rt_array_panic_oob: it
            // avoids SIGABRT and skips the atexit leak observer.
            eprintln!(
                "Runtime error: args index out of bounds: index {} not in range [0, {})",
                index,
                args.len()
            );
            use std::io::Write;
            let _ = std::io::stderr().flush();
            unsafe { libc::_exit(1) };
        }

        into_raw_ptr(MiriString::from_str(&args[index as usize]))
    }

    /// Returns the platform name (e.g. "macos", "linux", "windows").
    ///
    /// Allocates and returns a MiriString with RC=1. Caller must DecRef it.
    ///
    /// # Safety
    /// Always safe; returns a compile-time constant.
    #[no_mangle]
    pub extern "C" fn miri_rt_platform() -> *mut MiriString {
        let platform = std::env::consts::OS;
        into_raw_ptr(MiriString::from_str(platform))
    }

    /// Returns the status code of the last environment operation.
    /// 0 = success, 1-3 = error codes (see module doc).
    ///
    /// # Safety
    /// Always safe; no memory access.
    #[no_mangle]
    pub extern "C" fn miri_rt_env_status() -> i64 {
        ENV_STATUS.with(|s| *s.borrow())
    }

    /// Returns the OS error message of the last failure (status == 3).
    /// Empty string if the last operation succeeded or failed with a non-Other error.
    ///
    /// Allocates and returns a MiriString with RC=1. Caller must DecRef it.
    ///
    /// # Safety
    /// Always safe; thread-local storage access only.
    #[no_mangle]
    pub extern "C" fn miri_rt_env_error_message() -> *mut MiriString {
        ENV_ERROR_MESSAGE.with(|m| into_raw_ptr(MiriString::from_str(&m.borrow())))
    }
}
