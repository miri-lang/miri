// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_use_statement_local_module() {
    parser_test("
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
    parser_test("
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
    parser_test("
// Module with path
use System.Math as M
", vec![
        use_statement(
            import_path("System.Math".into()),
            opt_expr(identifier("M".into()))
        )
    ]);
}

#[test]
fn test_use_statement_wildcard() {
    parser_test("use System.IO.*", vec![
        use_statement(
            import_path_wildcard("System.IO"),
            None
        )
    ]);
}

#[test]
fn test_use_statement_multi_import() {
    parser_test("use System.{IO, Net, Text as T}", vec![
        use_statement(
            import_path_multi(
                "System",
                vec![
                    (identifier("IO"), None),
                    (identifier("Net"), None),
                    (identifier("Text"), opt_expr(identifier("T"))),
                ]
            ),
            None
        )
    ]);
}

#[test]
fn test_error_on_trailing_dot() {
    parser_error_test(
        "use MyModule.", 
        &SyntaxErrorKind::UnexpectedToken { 
            expected: "identifier".into(), 
            found: "end of file".into()
        }
    );
}

#[test]
fn test_error_on_missing_alias() {
    parser_error_test(
        "use MyModule as", 
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".into(),
            found: "end of file".into()
        }
    );
}

#[test]
fn test_error_on_invalid_multi_import() {
    parser_error_test("use System.{IO,", &SyntaxErrorKind::UnexpectedToken {
        expected: "identifier".to_string(),
        found: "end of file".to_string(),
    });
}
