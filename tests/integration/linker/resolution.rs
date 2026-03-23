// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::{assert_linker_error, ENV_MUTEX};
use std::env;

/// `MIRI_CC` takes priority: when it is set to a non-existent binary the build
/// must fail with a linker error that names the bogus path.
#[test]
fn test_linker_resolution_miri_cc() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let bogus = "/tmp/bogus_linker_path_that_does_not_exist";

    env::remove_var("CC");
    env::set_var("MIRI_CC", bogus);
    let result = std::panic::catch_unwind(|| assert_linker_error("0", bogus));
    env::remove_var("MIRI_CC");

    result.unwrap();
}

/// When `MIRI_CC` is absent, the `CC` environment variable is used as the
/// linker.  A bogus path must produce the same linker error.
#[test]
fn test_linker_resolution_cc() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let bogus = "/tmp/bogus_cc_path_that_does_not_exist";

    env::remove_var("MIRI_CC");
    env::set_var("CC", bogus);
    let result = std::panic::catch_unwind(|| assert_linker_error("0", bogus));
    env::remove_var("CC");

    result.unwrap();
}
