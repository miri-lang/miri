// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Time utilities for Miri runtime.

use once_cell::sync::Lazy;
use std::time::Instant;

static START_TIME: Lazy<Instant> = Lazy::new(Instant::now);

/// Stable FFI interface for time operations.
pub mod ffi {
    use super::START_TIME;
    use std::time::Duration;

    /// Returns nanoseconds elapsed since program start.
    #[no_mangle]
    pub extern "C" fn miri_rt_nanotime() -> i64 {
        START_TIME.elapsed().as_nanos() as i64
    }

    /// Sleeps for the specified number of nanoseconds.
    ///
    /// # Safety
    ///
    /// This function is always safe to call. A negative or zero `nanos` value
    /// returns immediately without sleeping.
    #[no_mangle]
    pub extern "C" fn miri_rt_sleep_nanos(nanos: i64) {
        if nanos > 0 {
            let duration = Duration::from_nanos(nanos as u64);
            std::thread::sleep(duration);
        }
    }
}
