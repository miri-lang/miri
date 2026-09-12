// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Unit tests for OS environment operations and thread safety.

use miri_runtime_core::{
    miri_rt_env_get, miri_rt_env_has, miri_rt_env_set, miri_rt_string_free, miri_rt_string_from_raw,
};
use std::thread;

#[test]
fn test_concurrent_env_operations() {
    let num_threads = 10;
    let iterations = 100;

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            thread::spawn(move || {
                let var_name = format!("MIRI_CONCURRENT_TEST_VAR_{}", t % 3);
                let name_ptr =
                    unsafe { miri_rt_string_from_raw(var_name.as_ptr(), var_name.len()) };

                for i in 0..iterations {
                    let val_str = format!("val_{}_{}", t, i);
                    let val_ptr =
                        unsafe { miri_rt_string_from_raw(val_str.as_ptr(), val_str.len()) };

                    unsafe {
                        let _ = miri_rt_env_set(name_ptr, val_ptr);
                        let _ = miri_rt_env_has(name_ptr);
                        let res_ptr = miri_rt_env_get(name_ptr);
                        miri_rt_string_free(res_ptr);
                        miri_rt_string_free(val_ptr);
                    }
                }

                unsafe {
                    miri_rt_string_free(name_ptr);
                }
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .expect("Thread panicked during concurrent env test");
    }
}
