//! Time utilities for Miri runtime.

use once_cell::sync::Lazy;
use std::time::Instant;

static START_TIME: Lazy<Instant> = Lazy::new(Instant::now);

/// Stable FFI interface for time operations.
pub mod ffi {
    use super::START_TIME;

    /// Returns nanoseconds elapsed since program start.
    #[no_mangle]
    pub extern "C" fn miri_rt_nanotime() -> i64 {
        START_TIME.elapsed().as_nanos() as i64
    }
}
