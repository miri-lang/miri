use miri_runtime_core::io::{
    miri_rt_eprint, miri_rt_eprintln, miri_rt_get_line_end, miri_rt_print, miri_rt_println,
};
use miri_runtime_core::string::miri_rt_string_free;

#[test]
fn test_line_ending() {
    unsafe {
        let line_end = miri_rt_get_line_end();

        #[cfg(windows)]
        assert_eq!((*line_end).as_str(), "\r\n");

        #[cfg(not(windows))]
        assert_eq!((*line_end).as_str(), "\n");

        miri_rt_string_free(line_end);
    }
}

#[test]
fn test_print_null() {
    unsafe {
        miri_rt_print(std::ptr::null());
        miri_rt_println(std::ptr::null());
        miri_rt_eprint(std::ptr::null());
        miri_rt_eprintln(std::ptr::null());
    }
}
