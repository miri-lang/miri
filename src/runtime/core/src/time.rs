use once_cell::sync::Lazy;
use std::time::Instant;

static START_TIME: Lazy<Instant> = Lazy::new(Instant::now);

#[no_mangle]
pub extern "C" fn miri_rt_nanotime() -> i64 {
    START_TIME.elapsed().as_nanos() as i64
}
