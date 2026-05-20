//! I/O primitives for the Miri runtime.
//!
//! Provides standard output and error stream operations callable from
//! compiled Miri code via FFI.

use std::cell::Cell;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Platform-agnostic over-allocation of `sigjmp_buf`. macOS aarch64 uses
/// `_JBLEN = 64` (520 bytes), x86_64 uses `_JBLEN = 48` (260 bytes), Linux
/// glibc allocates ~200 bytes. 768 bytes / 8-byte alignment covers every
/// supported target with room to spare and avoids depending on platform
/// `sigjmp_buf` bindings that the `libc` crate does not export uniformly.
#[repr(C, align(16))]
pub(super) struct SigJmpBuf(pub(super) [u64; 96]);

extern "C" {
    // On glibc, `sigsetjmp` is a macro that expands to `__sigsetjmp`; the bare
    // symbol does not exist in `libc.so`. macOS, BSDs, and musl export the
    // real `sigsetjmp` symbol, so only Linux+gnu needs the rename.
    #[cfg_attr(
        all(target_os = "linux", target_env = "gnu"),
        link_name = "__sigsetjmp"
    )]
    fn sigsetjmp(env: *mut SigJmpBuf, savemask: i32) -> i32;
    fn siglongjmp(env: *mut SigJmpBuf, val: i32) -> !;
}

thread_local! {
    /// Saved `SigJmpBuf` pointer for the innermost active
    /// `miri_rt_assert_panics` catch frame on this thread. When non-null,
    /// `miri_rt_panic` records the message in `CAUGHT_PANIC_MSG` and
    /// `siglongjmp`s back to the catch site instead of aborting.
    pub(super) static PANIC_CATCH_BUF: Cell<*mut SigJmpBuf> = const {
        Cell::new(std::ptr::null_mut())
    };
    /// Message captured by `miri_rt_panic` immediately before it `siglongjmp`s
    /// out of the user closure. Consumed by `miri_rt_assert_panics` after the
    /// jump returns. Leaks of intra-closure allocations are intentional: the
    /// process is expected to terminate soon after a test failure.
    pub(super) static CAUGHT_PANIC_MSG: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

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

    /// Prints a panic message to stderr and aborts the process.
    ///
    /// If a `miri_rt_assert_panics` catch frame is active on the current
    /// thread, stores the message in `CAUGHT_PANIC_MSG` and `siglongjmp`s
    /// back to the catch site instead of aborting. The catch site reads the
    /// message and decides whether to treat the panic as a test pass.
    ///
    /// # Safety
    /// - `s` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_panic(s: *const MiriString) {
        let msg: String = if s.is_null() {
            "explicit panic".to_string()
        } else {
            (*s).as_str().to_string()
        };
        let catch_buf = PANIC_CATCH_BUF.with(|c| c.get());
        if !catch_buf.is_null() {
            CAUGHT_PANIC_MSG.with(|m| *m.borrow_mut() = Some(msg));
            siglongjmp(catch_buf, 1);
        }
        eprintln!("Runtime error: {}", msg);
        die();
    }

    /// Helper that formats the standard "assertion failed at <location>" prefix.
    unsafe fn format_assert_prefix(location: *const MiriString) -> String {
        if location.is_null() {
            "assertion failed".to_string()
        } else {
            format!("assertion failed at {}", (*location).as_str())
        }
    }

    /// Helper that returns the user message suffix (": <msg>") if non-empty.
    unsafe fn user_msg_suffix(user_msg: *const MiriString) -> String {
        if user_msg.is_null() {
            return String::new();
        }
        let s = (*user_msg).as_str();
        if s.is_empty() {
            String::new()
        } else {
            format!(": {}", s)
        }
    }

    /// Clean-exit termination for user-facing runtime errors.
    ///
    /// Flushes stderr (so the preceding `eprintln!` is visible), then calls
    /// `libc::_exit(1)`. Skips atexit handlers — so the `MIRI_LEAK_CHECK`
    /// observer does not fire on intentional error exits, and on macOS the
    /// kernel does not invoke `ReportCrash`. Compared with
    /// `std::process::abort()` (which raises SIGABRT), this keeps test
    /// processes out of `~/Library/Logs/DiagnosticReports` and avoids the
    /// crash-daemon contention that slows parallel test runs.
    fn die() -> ! {
        let _ = io::stderr().flush();
        unsafe { libc::_exit(1) }
    }

    /// Reports a failed `assert(cond)` and aborts.
    ///
    /// # Safety
    /// - `user_msg` and `location` must be valid pointers to `MiriString`s or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_assert_fail(
        user_msg: *const MiriString,
        location: *const MiriString,
    ) {
        let prefix = format_assert_prefix(location);
        let suffix = user_msg_suffix(user_msg);
        eprintln!("Runtime error: {}{}", prefix, suffix);
        die();
    }

    /// Reports a failed `assert_eq(actual, expected)` and aborts.
    ///
    /// # Safety
    /// - `expected_str`, `actual_str`, `user_msg`, and `location` must be valid
    ///   `MiriString` pointers or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_assert_eq_fail(
        expected_str: *const MiriString,
        actual_str: *const MiriString,
        user_msg: *const MiriString,
        location: *const MiriString,
    ) {
        let prefix = format_assert_prefix(location);
        let expected_s = if expected_str.is_null() {
            "<null>"
        } else {
            (*expected_str).as_str()
        };
        let actual_s = if actual_str.is_null() {
            "<null>"
        } else {
            (*actual_str).as_str()
        };
        let suffix = user_msg_suffix(user_msg);
        eprintln!(
            "Runtime error: {}: expected {}, got {}{}",
            prefix, expected_s, actual_s, suffix
        );
        die();
    }

    /// Reports a failed `assert_ne(a, b)` and aborts.
    ///
    /// # Safety
    /// - `value_str`, `user_msg`, and `location` must be valid `MiriString`
    ///   pointers or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_assert_ne_fail(
        value_str: *const MiriString,
        user_msg: *const MiriString,
        location: *const MiriString,
    ) {
        let prefix = format_assert_prefix(location);
        let val_s = if value_str.is_null() {
            "<null>"
        } else {
            (*value_str).as_str()
        };
        let suffix = user_msg_suffix(user_msg);
        eprintln!(
            "Runtime error: {}: values must differ, both were {}{}",
            prefix, val_s, suffix
        );
        die();
    }

    /// Reports an integer divide-by-zero error and `_exit(1)`s.
    ///
    /// Called from compiled Miri code in place of a Cranelift `trapz`
    /// hardware-trap instruction so the process terminates cleanly without
    /// raising SIGTRAP/SIGILL. Keeps macOS `ReportCrash` out of the picture.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_div_by_zero_panic() {
        eprintln!("Runtime error: division by zero");
        die();
    }

    /// Invokes the zero-argument closure `closure_ptr` and verifies it panics.
    ///
    /// The closure layout is `[fn_ptr][dtor_ptr][captures...]`; `closure_ptr`
    /// points to the start of the closure payload, which is also the
    /// environment pointer passed to the closure as its implicit first
    /// argument.
    ///
    /// Behavior:
    /// - If the closure returns normally → emits an assertion-failed message
    ///   at `location` and aborts.
    /// - If the closure panics → captures the panic message string. If
    ///   `expected` is non-null and non-empty, additionally checks that the
    ///   captured message contains `expected` as a substring; aborts with a
    ///   diagnostic if it doesn't. Otherwise returns normally.
    ///
    /// Note: any heap allocations made inside the closure between entry and
    /// panic are leaked, because Perceus drop glue is skipped on the unwind
    /// path. This is acceptable for test-only code; the process is expected
    /// to terminate soon after.
    ///
    /// # Safety
    /// - `closure_ptr` must point to a valid Miri closure payload whose first
    ///   word is the closure function pointer of signature
    ///   `extern "C" fn(*mut u8)`.
    /// - `expected` and `location` must be valid pointers to `MiriString`s or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_assert_panics(
        closure_ptr: *mut u8,
        expected: *const MiriString,
        location: *const MiriString,
    ) {
        if closure_ptr.is_null() {
            eprintln!("Runtime error: assert_panics: null closure");
            die();
        }

        let loc_str: String = if location.is_null() {
            "<unknown location>".to_string()
        } else {
            (*location).as_str().to_string()
        };

        // Stack-allocated jump buffer. We `sigsetjmp` here, install the buf
        // pointer in TLS, then invoke the closure. If the closure calls
        // `miri_rt_panic`, the panic helper stores the message and
        // `siglongjmp`s back to this frame with a non-zero return value.
        //
        // The Rust drop glue on this frame is trivial (`MaybeUninit<sigjmp_buf>`
        // is `Drop`-less, and the only owned strings are constructed AFTER the
        // jump returns), so longjmp does not skip any required Drop call.
        let mut buf: std::mem::MaybeUninit<SigJmpBuf> = std::mem::MaybeUninit::uninit();
        let buf_ptr: *mut SigJmpBuf = buf.as_mut_ptr();

        let prev_buf = PANIC_CATCH_BUF.with(|c| c.replace(buf_ptr));
        CAUGHT_PANIC_MSG.with(|m| m.borrow_mut().take());

        let jump_val = sigsetjmp(buf_ptr, 0);
        if jump_val == 0 {
            // First-time entry: invoke the closure. The closure's first
            // argument is `env_ptr`, which equals `closure_ptr` (the payload
            // pointer); the function pointer lives at `payload[0]`.
            let fn_ptr_addr = *(closure_ptr as *const usize);
            let f: extern "C" fn(*mut u8) = std::mem::transmute(fn_ptr_addr);
            f(closure_ptr);

            // Closure returned without panicking — restore catch slot and
            // report failure.
            PANIC_CATCH_BUF.with(|c| c.set(prev_buf));
            eprintln!(
                "Runtime error: {}: assertion failed: assert_panics: closure did not panic",
                loc_str
            );
            die();
        }

        // siglongjmp landed here. Restore the previous catch frame so nested
        // assert_panics work, then validate the captured message against
        // `expected` if one was provided.
        PANIC_CATCH_BUF.with(|c| c.set(prev_buf));
        let captured: String = CAUGHT_PANIC_MSG
            .with(|m| m.borrow_mut().take())
            .unwrap_or_default();

        if !expected.is_null() {
            let exp = (*expected).as_str();
            if !exp.is_empty() && !captured.contains(exp) {
                eprintln!(
                    "Runtime error: {}: assertion failed: assert_panics: expected panic containing \"{}\", got \"{}\"",
                    loc_str, exp, captured
                );
                die();
            }
        }
    }
}
