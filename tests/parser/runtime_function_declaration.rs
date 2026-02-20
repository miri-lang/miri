// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::common::RuntimeKind;
use miri::ast::factory::{
    parameter, runtime_function_declaration, type_expr_non_null, type_int, type_string,
};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_runtime_function_declaration_default_runtime() {
    parser_test(
        "runtime fn miri_rt_alloc(size int) int\n",
        vec![runtime_function_declaration(
            RuntimeKind::Core,
            "miri_rt_alloc",
            vec![parameter(
                "size".into(),
                type_expr_non_null(type_int()),
                None,
                None,
            )],
            Some(Box::new(type_expr_non_null(type_int()))),
        )],
    );
}

#[test]
fn test_runtime_function_declaration_explicit_core() {
    parser_test(
        "runtime \"core\" fn miri_rt_alloc(size int) int\n",
        vec![runtime_function_declaration(
            RuntimeKind::Core,
            "miri_rt_alloc",
            vec![parameter(
                "size".into(),
                type_expr_non_null(type_int()),
                None,
                None,
            )],
            Some(Box::new(type_expr_non_null(type_int()))),
        )],
    );
}

#[test]
fn test_runtime_function_declaration_no_params_no_return() {
    parser_test(
        "runtime fn miri_rt_init()\n",
        vec![runtime_function_declaration(
            RuntimeKind::Core,
            "miri_rt_init",
            vec![],
            None,
        )],
    );
}

#[test]
fn test_runtime_function_declaration_multiple_params() {
    parser_test(
        "runtime fn miri_rt_copy(src String, dst String) int\n",
        vec![runtime_function_declaration(
            RuntimeKind::Core,
            "miri_rt_copy",
            vec![
                parameter("src".into(), type_expr_non_null(type_string()), None, None),
                parameter("dst".into(), type_expr_non_null(type_string()), None, None),
            ],
            Some(Box::new(type_expr_non_null(type_int()))),
        )],
    );
}

#[test]
fn test_runtime_function_declaration_unknown_runtime() {
    parser_error_test(
        "runtime \"gpu\" fn miri_rt_launch()\n",
        &SyntaxErrorKind::UnknownRuntime {
            name: "gpu".to_string(),
        },
    );
}

#[test]
fn test_runtime_function_declaration_visibility_rejected() {
    parser_error_test(
        "public runtime fn miri_rt_alloc(size int) int\n",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a declaration (runtime functions cannot have visibility modifiers)"
                .to_string(),
            found: "runtime".to_string(),
        },
    );
}

#[test]
fn test_runtime_function_declaration_private_rejected() {
    parser_error_test(
        "private runtime fn miri_rt_alloc(size int) int\n",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a declaration (runtime functions cannot have visibility modifiers)"
                .to_string(),
            found: "runtime".to_string(),
        },
    );
}
