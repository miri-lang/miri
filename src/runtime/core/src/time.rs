use std::time::Instant;
use once_cell::sync::Lazy;

static START_TIME: Lazy<Instant> = Lazy::new(Instant::now);

#[no_mangle]
pub extern "C" fn miri_rt_nanotime() -> i64 {
    START_TIME.elapsed().as_nanos() as i64
}
