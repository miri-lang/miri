// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Filesystem operations for the Miri runtime.
//!
//! Provides file reading, writing, and directory operations callable from
//! compiled Miri code via FFI.
//!
//! # Error Handling via Status Channel
//!
//! Since `runtime "core" fn` cannot return `Result`, all fallible operations
//! set a thread-local status code and return a default value. The status code
//! and associated error message are thread-local, and their values are only
//! meaningful when read immediately after a fallible call returns. No operation
//! that could set the fs status may run between the fallible call and the status
//! read; otherwise the pairing is lost and the caller will observe stale or
//! unrelated error information.
//!
//! Status codes (matches fs.mi):
//! - 0 = success
//! - 1 = NotFound
//! - 2 = PermissionDenied
//! - 3 = AlreadyExists
//! - 4 = NotADirectory (exists but not the expected kind)
//! - 5 = InvalidData (e.g. non-UTF-8)
//! - 6 = Other (OS error message in `miri_rt_fs_error_message()`)

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::Path;

use crate::string::{into_raw_ptr, MiriString};

thread_local! {
    /// Thread-local status code for the last filesystem operation.
    /// 0 = success, 1-6 = error codes (see module doc).
    static FS_STATUS: RefCell<i64> = const { RefCell::new(0) };
    /// Holds the OS error message for the last failure (populated only when status == 6).
    static FS_ERROR_MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Sets the thread-local status code and clears the error message on success.
fn set_status(code: i64, msg: String) {
    FS_STATUS.with(|s| {
        *s.borrow_mut() = code;
    });
    if code != 0 {
        FS_ERROR_MESSAGE.with(|m| {
            *m.borrow_mut() = msg;
        });
    } else {
        FS_ERROR_MESSAGE.with(|m| {
            m.borrow_mut().clear();
        });
    }
}

/// Maps an `io::Error` to a status code and message string.
/// Preserves the `InvalidData` distinction for reads: only `read_to_string`
/// can produce `InvalidData`, while writes cannot.
fn map_io_error(e: &io::Error, path: &str, is_read: bool) -> (i64, String) {
    match e.kind() {
        io::ErrorKind::NotFound => (1, path.to_string()),
        io::ErrorKind::PermissionDenied => (2, path.to_string()),
        io::ErrorKind::InvalidData if is_read => (5, path.to_string()),
        _ => (6, e.to_string()),
    }
}

/// Returns whether a path exists and is a directory.
fn is_dir(path: &str) -> bool {
    if let Ok(metadata) = fs::metadata(path) {
        metadata.is_dir()
    } else {
        false
    }
}

pub mod ffi {
    use super::*;

    /// Returns the status code of the last filesystem operation.
    /// 0 = success, 1-6 = error (see module doc for codes).
    #[no_mangle]
    pub extern "C" fn miri_rt_fs_status() -> i64 {
        FS_STATUS.with(|s| *s.borrow())
    }

    /// Returns the OS error message of the last failure (status == 6).
    /// Empty string if the last operation succeeded or failed with a non-Other error.
    ///
    /// Allocates and returns a MiriString with RC=1. Caller must DecRef it.
    #[no_mangle]
    pub extern "C" fn miri_rt_fs_error_message() -> *mut MiriString {
        FS_ERROR_MESSAGE.with(|m| {
            let msg = m.borrow().clone();
            into_raw_ptr(MiriString::from_str(&msg))
        })
    }

    /// Checks whether a file or directory exists at the given path.
    /// Returns 1 if exists, 0 if not. Never fails (always status = 0).
    ///
    /// # Safety
    /// The caller must ensure that `path` is a valid pointer to a MiriString.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_exists(path: *const MiriString) -> i64 {
        if path.is_null() {
            set_status(0, String::new());
            return 0;
        }

        let path_str = (*path).as_str();
        set_status(0, String::new());
        if Path::new(path_str).exists() {
            1
        } else {
            0
        }
    }

    /// Reads the contents of a file at the given path into a newly allocated MiriString.
    /// Returns an empty string and sets status on failure.
    ///
    /// Status codes:
    /// - 0 = success
    /// - 1 = NotFound
    /// - 2 = PermissionDenied
    /// - 4 = NotADirectory (path is a directory, not a file)
    /// - 5 = InvalidData (non-UTF-8 content)
    /// - 6 = Other (OS error)
    ///
    /// # Safety
    /// The caller must ensure that `path` is a valid pointer to a MiriString.
    /// The returned pointer must be DecRef'd by the caller when no longer needed.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_read_file(path: *const MiriString) -> *mut MiriString {
        if path.is_null() {
            set_status(6, "null path pointer".to_string());
            return into_raw_ptr(MiriString::from_str(""));
        }

        let path_str = (*path).as_str();

        // Check if it's a directory
        if is_dir(path_str) {
            set_status(4, path_str.to_string());
            return into_raw_ptr(MiriString::from_str(""));
        }

        match fs::read_to_string(path_str) {
            Ok(contents) => {
                set_status(0, String::new());
                into_raw_ptr(MiriString::from_str(&contents))
            }
            Err(e) => {
                let (code, msg) = map_io_error(&e, path_str, true);
                set_status(code, msg);
                into_raw_ptr(MiriString::from_str(""))
            }
        }
    }

    /// Writes the given contents to a file at the given path, truncating if it exists.
    /// Returns the number of bytes written on success, -1 on failure (status set accordingly).
    ///
    /// Status codes:
    /// - 0 = success
    /// - 1 = NotFound (parent directory does not exist)
    /// - 2 = PermissionDenied
    /// - 4 = NotADirectory (path exists but is a directory, not a file)
    /// - 6 = Other (OS error)
    ///
    /// # Safety
    /// The caller must ensure that both `path` and `contents` are valid pointers to MiriStrings.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_write_file(
        path: *const MiriString,
        contents: *const MiriString,
    ) -> i64 {
        if path.is_null() || contents.is_null() {
            set_status(6, "null pointer".to_string());
            return -1;
        }

        let path_str = (*path).as_str();
        let contents_str = (*contents).as_str();
        let byte_count = contents_str.len() as i64;

        // Check if path exists and is a directory
        if is_dir(path_str) {
            set_status(4, path_str.to_string());
            return -1;
        }

        match fs::write(path_str, contents_str) {
            Ok(()) => {
                set_status(0, String::new());
                byte_count
            }
            Err(e) => {
                let (code, msg) = map_io_error(&e, path_str, false);
                set_status(code, msg);
                -1
            }
        }
    }

    /// Appends the given contents to a file at the given path, creating it if it doesn't exist.
    /// Returns the number of bytes written on success, -1 on failure (status set accordingly).
    ///
    /// Status codes:
    /// - 0 = success
    /// - 1 = NotFound (parent directory does not exist)
    /// - 2 = PermissionDenied
    /// - 4 = NotADirectory (path exists but is a directory, not a file)
    /// - 6 = Other (OS error)
    ///
    /// # Safety
    /// The caller must ensure that both `path` and `contents` are valid pointers to MiriStrings.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_append_file(
        path: *const MiriString,
        contents: *const MiriString,
    ) -> i64 {
        if path.is_null() || contents.is_null() {
            set_status(6, "null pointer".to_string());
            return -1;
        }

        let path_str = (*path).as_str();
        let contents_str = (*contents).as_str();
        let byte_count = contents_str.len() as i64;

        // Check if path exists and is a directory
        if is_dir(path_str) {
            set_status(4, path_str.to_string());
            return -1;
        }

        match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path_str)
        {
            Ok(mut file) => {
                use std::io::Write;
                match file.write_all(contents_str.as_bytes()) {
                    Ok(()) => {
                        set_status(0, String::new());
                        byte_count
                    }
                    Err(e) => {
                        let (code, msg) = map_io_error(&e, path_str, false);
                        set_status(code, msg);
                        -1
                    }
                }
            }
            Err(e) => {
                let (code, msg) = map_io_error(&e, path_str, false);
                set_status(code, msg);
                -1
            }
        }
    }

    /// Lists the entries (files and directories) in a directory, returning their names.
    /// Returns an empty list and sets status on failure.
    ///
    /// Status codes:
    /// - 0 = success
    /// - 1 = NotFound (directory doesn't exist)
    /// - 2 = PermissionDenied
    /// - 4 = NotADirectory (path exists but is not a directory)
    /// - 6 = Other (OS error)
    ///
    /// # Safety
    /// The caller must ensure that `path` is a valid pointer to a MiriString.
    /// The returned pointer must be DecRef'd by the caller when no longer needed.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_list_dir(
        path: *const MiriString,
    ) -> *mut crate::list::MiriList {
        if path.is_null() {
            set_status(6, "null path pointer".to_string());
            let list = crate::miri_rt_list_new(std::mem::size_of::<*mut u8>());
            return list;
        }

        let path_str = (*path).as_str();

        // Check if the path exists
        if !Path::new(path_str).exists() {
            set_status(1, path_str.to_string());
            let list = crate::miri_rt_list_new(std::mem::size_of::<*mut u8>());
            return list;
        }

        // Check if it's a directory
        if !is_dir(path_str) {
            set_status(4, path_str.to_string());
            let list = crate::miri_rt_list_new(std::mem::size_of::<*mut u8>());
            return list;
        }

        // Create a list to hold the entry names (element size = pointer)
        let list = crate::miri_rt_list_new(std::mem::size_of::<*mut u8>());
        if list.is_null() {
            set_status(6, "allocation failed".to_string());
            return list;
        }

        match fs::read_dir(path_str) {
            Ok(entries) => {
                for entry_result in entries {
                    let Ok(entry) = entry_result else {
                        continue;
                    };
                    let Ok(name) = entry.file_name().into_string() else {
                        continue;
                    };
                    let name_str = into_raw_ptr(MiriString::from_str(&name));
                    if !name_str.is_null() {
                        // The string is freshly allocated at RC=1 and
                        // handed to the list, so that count *is* the
                        // list's ownership. Adding one here would
                        // outlive the matching DecRef in elem_drop_fn.
                        crate::miri_rt_list_push(list, name_str as usize);
                    }
                }
                // Set elem_drop_fn so removed elements are DecRef'd
                crate::miri_rt_list_set_elem_drop_fn(
                    list,
                    crate::miri_rt_list_decref_element as *const () as usize,
                );
                set_status(0, String::new());
            }
            Err(e) => {
                let (code, msg) = map_io_error(&e, path_str, false);
                set_status(code, msg);
            }
        }

        list
    }

    /// Creates a directory at the given path, creating missing parents as needed.
    /// Idempotent: if the directory already exists, succeeds with status = 0 and returns 0.
    /// Returns 1 if the directory was newly created, 0 if it already existed, -1 on failure (status set accordingly).
    ///
    /// Status codes:
    /// - 0 = success (created or already exists as a directory)
    /// - 1 = NotFound (parent directory does not exist)
    /// - 2 = PermissionDenied
    /// - 3 = AlreadyExists (path exists but is not a directory)
    /// - 6 = Other (OS error)
    ///
    /// # Safety
    /// The caller must ensure that `path` is a valid pointer to a MiriString.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_create_dir(path: *const MiriString) -> i64 {
        if path.is_null() {
            set_status(6, "null path pointer".to_string());
            return -1;
        }

        let path_str = (*path).as_str();

        // Check if the path exists, fetching metadata once
        let metadata = fs::metadata(path_str);
        let already_exists = matches!(&metadata, Ok(m) if m.is_dir());

        if metadata.is_ok() && !already_exists {
            set_status(3, path_str.to_string());
            return -1;
        }

        match fs::create_dir_all(path_str) {
            Ok(()) => {
                set_status(0, String::new());
                if already_exists {
                    0
                } else {
                    1
                }
            }
            Err(e) => {
                let (code, msg) = map_io_error(&e, path_str, false);
                set_status(code, msg);
                -1
            }
        }
    }

    /// Deletes a regular file at the given path.
    /// Fails if the path is a directory or doesn't exist.
    /// Returns 1 on success, -1 on failure (status set accordingly).
    ///
    /// Note: Success always returns 1; failure always returns -1. The bool return type
    /// is preserved in the public API for consistency, but there is no "soft" answer to
    /// "was something deleted" — it either succeeded with 1 or failed with -1 and status code.
    ///
    /// Status codes:
    /// - 0 = success
    /// - 1 = NotFound
    /// - 2 = PermissionDenied
    /// - 4 = NotADirectory (path is a directory, not a file)
    /// - 6 = Other (OS error)
    ///
    /// # Safety
    /// The caller must ensure that `path` is a valid pointer to a MiriString.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_fs_delete(path: *const MiriString) -> i64 {
        if path.is_null() {
            set_status(6, "null path pointer".to_string());
            return -1;
        }

        let path_str = (*path).as_str();

        // Check if the path exists, fetching metadata once
        match fs::metadata(path_str) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    set_status(4, path_str.to_string());
                    return -1;
                }
            }
            Err(e) => {
                let (code, msg) = if e.kind() == io::ErrorKind::NotFound {
                    (1, path_str.to_string())
                } else {
                    (6, e.to_string())
                };
                set_status(code, msg);
                return -1;
            }
        }

        match fs::remove_file(path_str) {
            Ok(()) => {
                set_status(0, String::new());
                1
            }
            Err(e) => {
                let (code, msg) = map_io_error(&e, path_str, false);
                set_status(code, msg);
                -1
            }
        }
    }

    /// Returns the current working directory as a newly allocated MiriString.
    /// Returns an empty string and sets status on failure.
    ///
    /// Status codes:
    /// - 0 = success
    /// - 6 = Other (OS error)
    #[no_mangle]
    pub extern "C" fn miri_rt_fs_cwd() -> *mut MiriString {
        match std::env::current_dir() {
            Ok(path_buf) => {
                if let Some(path_str) = path_buf.to_str() {
                    set_status(0, String::new());
                    into_raw_ptr(MiriString::from_str(path_str))
                } else {
                    set_status(6, "path contains invalid UTF-8".to_string());
                    into_raw_ptr(MiriString::from_str(""))
                }
            }
            Err(e) => {
                set_status(6, e.to_string());
                into_raw_ptr(MiriString::from_str(""))
            }
        }
    }
}

// Re-export FFI functions at module level for backward-compatible access
pub use ffi::*;
