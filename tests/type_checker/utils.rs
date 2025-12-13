// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::pipeline::Pipeline;

pub fn type_check_test(source: &str) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => {},
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    }
}

pub fn type_check_error_test(source: &str, expected_error_part: &str) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Type check should have failed but succeeded"),
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains(expected_error_part), "Error message '{}' did not contain '{}'", msg, expected_error_part);
        }
    }
}