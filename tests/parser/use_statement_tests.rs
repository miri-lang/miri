// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_use_statement_local_module() {
    parse_test("
// Local module 
use Calc
", vec![
        use_statement(
            import_path("Calc".into()),
            None
        )
    ]);
}

#[test]
fn test_use_statement_multiple_segments() {
    parse_test("
// Local module with path
use MyProject.Path.SomeModule
", vec![
        use_statement(
            import_path("MyProject.Path.SomeModule".into()),
            None
        )
    ]);
}

#[test]
fn test_use_statement_alias() {
    parse_test("
// Module with path
use System.Math as M
", vec![
        use_statement(
            import_path("System.Math".into()),
            opt_expr(identifier("M".into()))
        )
    ]);
}
