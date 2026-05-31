// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::script::{patch_main_return, wrap_script_in_main};
use miri::ast::Program;

#[test]
fn patch_main_return_handles_empty_program() {
    let mut prog = Program { body: Vec::new() };
    patch_main_return(&mut prog);
    // Should not crash or modify an empty program
    assert!(prog.body.is_empty());
}

#[test]
fn wrap_script_in_main_handles_empty_program() {
    let mut prog = Program { body: Vec::new() };
    wrap_script_in_main(&mut prog);
    // Should synthesize a main function for an empty program
    assert_eq!(prog.body.len(), 1);
}
