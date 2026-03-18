// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::pipeline::{BuildOptions, Pipeline};
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;

/// Mutex to serialize tests that mutate process-wide environment variables.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_linker_resolution_miri_cc() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let source = "0";
    let pipeline = Pipeline::new();

    // Set MIRI_CC to a non-existent path
    let bogus_linker = "/tmp/bogus_linker_path_that_does_not_exist";
    env::remove_var("CC");
    env::set_var("MIRI_CC", bogus_linker);

    let opts = BuildOptions {
        out_path: Some(PathBuf::from("/tmp/miri_test_output")),
        ..Default::default()
    };

    let result = pipeline.build(source, &opts);

    // Clean up
    env::remove_var("MIRI_CC");

    match result {
        Err(miri::error::compiler::CompilerError::Codegen(msg)) => {
            assert!(
                msg.contains("Failed to run linker"),
                "Error message should mention linker failure, got: {}",
                msg
            );
            assert!(
                msg.contains(bogus_linker),
                "Error message should contain the bogus linker path, got: {}",
                msg
            );
        }
        _ => panic!(
            "Expected Codegen error due to bogus linker, got {:?}",
            result
        ),
    }
}

#[test]
fn test_linker_resolution_cc() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let source = "0";
    let pipeline = Pipeline::new();

    // Ensure MIRI_CC is NOT set
    env::remove_var("MIRI_CC");

    // Set CC to a non-existent path
    let bogus_linker = "/tmp/bogus_cc_path_that_does_not_exist";
    env::set_var("CC", bogus_linker);

    let opts = BuildOptions {
        out_path: Some(PathBuf::from("/tmp/miri_test_output_cc")),
        ..Default::default()
    };

    let result = pipeline.build(source, &opts);

    // Clean up
    env::remove_var("CC");

    match result {
        Err(miri::error::compiler::CompilerError::Codegen(msg)) => {
            assert!(
                msg.contains("Failed to run linker"),
                "Error message should mention linker failure, got: {}",
                msg
            );
            assert!(
                msg.contains(bogus_linker),
                "Error message should contain the bogus linker path, got: {}",
                msg
            );
        }
        _ => panic!(
            "Expected Codegen error due to bogus linker, got {:?}",
            result
        ),
    }
}
