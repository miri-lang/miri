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

/// Relative linker paths containing directory separators must be rejected
/// to prevent CWD/relative path hijacking vulnerabilities.
#[test]
fn test_linker_resolution_rejects_relative_path_with_separators() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let relative_path = "./bogus_relative_cc";

    env::remove_var("CC");
    env::set_var("MIRI_CC", relative_path);

    let pipeline = miri::pipeline::Pipeline::new();
    let opts = miri::pipeline::BuildOptions {
        out_path: Some(std::path::PathBuf::from("/tmp/miri_linker_test_output")),
        ..Default::default()
    };

    let result = pipeline.build("0", &opts);
    env::remove_var("MIRI_CC");

    match result {
        Err(miri::error::compiler::CompilerError::Codegen(msg)) => {
            assert!(
                msg.contains("relative paths containing directory separators are not allowed"),
                "Expected error message for relative linker path rejection, got: {}",
                msg
            );
        }
        other => panic!(
            "Expected Codegen error rejecting relative linker path, got: {:?}",
            other
        ),
    }
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
