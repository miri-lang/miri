// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_use_statement_package_module() {
    parser_test(
        "
// Package module 
use calc
",
        vec![use_statement(import_path("calc".into()), None)],
    );
}

#[test]
fn test_use_statement_multiple_segments() {
    parser_test(
        "
// Package module with path
use calc.path.some_module
",
        vec![use_statement(
            import_path("calc.path.some_module".into()),
            None,
        )],
    );
}

#[test]
fn test_use_statement_alias() {
    parser_test(
        "
// System module with alias
use system.math as M
",
        vec![use_statement(
            import_path("system.math".into()),
            opt_expr(identifier("M".into())),
        )],
    );
}

#[test]
fn test_use_statement_wildcard() {
    parser_test(
        "use system.io.*",
        vec![use_statement(import_path_wildcard("system.io"), None)],
    );
}

#[test]
fn test_use_statement_multi_import() {
    parser_test(
        "use system.{io, net, text as T}",
        vec![use_statement(
            import_path_multi(
                "system",
                vec![
                    (identifier("io"), None),
                    (identifier("net"), None),
                    (identifier("text"), opt_expr(identifier("T"))),
                ],
            ),
            None,
        )],
    );
}

#[test]
fn test_error_on_trailing_dot() {
    parser_error_test(
        "use calc.path.some_module.",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".into(),
            found: "end of file".into(),
        },
    );
}

#[test]
fn test_error_on_missing_alias() {
    parser_error_test(
        "use calc.path.some_module as",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".into(),
            found: "end of file".into(),
        },
    );
}

#[test]
fn test_error_on_invalid_multi_import() {
    parser_error_test(
        "use system.{io,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        },
    );
}

#[test]
fn test_use_multiple_statements() {
    parser_test(
        "
use system.math
use local.users.user
use system.io.{print, println}
use system.{io, net as network}
",
        vec![
            use_statement(import_path("system.math".into()), None),
            use_statement(import_path("local.users.user".into()), None),
            use_statement(
                import_path_multi(
                    "system.io",
                    vec![(identifier("print"), None), (identifier("println"), None)],
                ),
                None,
            ),
            use_statement(
                import_path_multi(
                    "system",
                    vec![
                        (identifier("io"), None),
                        (identifier("net"), Some(Box::new(identifier("network")))),
                    ],
                ),
                None,
            ),
        ],
    );
}
