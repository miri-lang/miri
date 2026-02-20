// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    binary, block, boolean_literal, empty_statement, expression_statement, func, generic_type,
    generic_type_with_kind, guard, identifier, int_literal_expression, parameter, return_statement,
    type_bool, type_custom, type_expr_non_null, type_int, type_list, type_string,
};
use miri::ast::types::TypeDeclarationKind;
use miri::ast::{opt_expr, BinaryOp, GuardOp};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_function_declaration() {
    parser_test(
        "
fn square(x int)
    x * x
",
        vec![func("square")
            .params(vec![parameter(
                "x".into(),
                type_expr_non_null(type_int()),
                None,
                None,
            )])
            .build(block(vec![expression_statement(binary(
                identifier("x"),
                BinaryOp::Mul,
                identifier("x"),
            ))]))],
    );
}

#[test]
fn test_function_declaration_with_guard() {
    parser_test(
        "
fn square(x int > 0)
    x * x
",
        vec![func("square")
            .params(vec![parameter(
                "x".into(),
                type_expr_non_null(type_int()),
                opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                None,
            )])
            .build(block(vec![expression_statement(binary(
                identifier("x"),
                BinaryOp::Mul,
                identifier("x"),
            ))]))],
    );
}

#[test]
fn test_inline_function_declaration_with_guard() {
    parser_test(
        "
fn square(x int > 0) int: x * x
",
        vec![func("square")
            .params(vec![parameter(
                "x".into(),
                type_expr_non_null(type_int()),
                opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                None,
            )])
            .return_type(type_expr_non_null(type_int()))
            .build(expression_statement(binary(
                identifier("x"),
                BinaryOp::Mul,
                identifier("x"),
            )))],
    );
}

#[test]
fn test_function_no_parameters() {
    parser_test(
        "
fn get_answer() int: 42
",
        vec![func("get_answer")
            .return_type(type_expr_non_null(type_int()))
            .build(expression_statement(int_literal_expression(42)))],
    );
}

#[test]
fn test_function_multiple_parameters() {
    parser_test(
        "
fn add(a int, b int)
    return a + b
",
        vec![func("add")
            .params(vec![
                parameter("a".into(), type_expr_non_null(type_int()), None, None),
                parameter("b".into(), type_expr_non_null(type_int()), None, None),
            ])
            .build(block(vec![return_statement(opt_expr(binary(
                identifier("a"),
                BinaryOp::Add,
                identifier("b"),
            )))]))],
    );
}

#[test]
fn test_function_untyped_parameter() {
    parser_error_test(
        "
fn process(data)
    // do something
",
        &SyntaxErrorKind::MissingTypeExpression,
    );
}

#[test]
fn test_function_empty_body_block() {
    parser_test(
        "
fn no_op()
    // This function does nothing
",
        vec![func("no_op").build(empty_statement())],
    );
}

#[test]
fn test_function_empty_body_inline() {
    parser_test(
        "
fn no_op_inline(): // This function also does nothing
",
        vec![func("no_op_inline").build(empty_statement())],
    );
}

#[test]
fn test_error_function_missing_parens() {
    parser_error_test(
        "fn my_func int: 42",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "(".to_string(),
            found: "identifier".to_string(),
        },
    );
}

#[test]
fn test_error_function_invalid_parameter() {
    parser_error_test(
        "fn my_func(123)",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "int".to_string(),
        },
    );
}

#[test]
fn test_function_with_single_generic_type() {
    parser_test(
        "
fn my_func<T>()
    // body
",
        vec![func("my_func")
            .generics(vec![generic_type("T", None)])
            .params(vec![])
            .build(empty_statement())],
    );
}

#[test]
fn test_function_with_multiple_generic_types() {
    parser_test(
        "
fn my_func<K, V>()
    // body
",
        vec![func("my_func")
            .generics(vec![generic_type("K", None), generic_type("V", None)])
            .params(vec![])
            .build(empty_statement())],
    );
}

#[test]
fn test_function_with_constrained_generic_type() {
    parser_test(
        "
fn my_func<T extends SomeClass>()
    // body
",
        vec![func("my_func")
            .generics(vec![generic_type_with_kind(
                "T",
                opt_expr(type_expr_non_null(type_custom("SomeClass", None))),
                TypeDeclarationKind::Extends,
            )])
            .params(vec![])
            .build(empty_statement())],
    );
}

#[test]
fn test_function_with_mixed_generic_types() {
    parser_test(
        "
fn my_func<K, V extends SomeTrait>()
    // body
",
        vec![func("my_func")
            .generics(vec![
                generic_type("K", None),
                generic_type_with_kind(
                    "V",
                    opt_expr(type_expr_non_null(type_custom("SomeTrait", None))),
                    TypeDeclarationKind::Extends,
                ),
            ])
            .params(vec![])
            .build_empty_body()],
    );
}

#[test]
fn test_function_using_generic_types() {
    parser_test(
        "
fn process<T>(data T) T: data
",
        vec![func("process")
            .generics(vec![generic_type("T", None)])
            .params(vec![parameter(
                "data".into(),
                type_expr_non_null(type_custom("T", None)),
                None,
                None,
            )])
            .return_type(type_expr_non_null(type_custom("T", None)))
            .build(expression_statement(identifier("data")))],
    );
}

#[test]
fn test_error_function_unclosed_generics() {
    parser_error_test("fn my_func<T", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_function_empty_generics() {
    parser_error_test(
        "fn my_func<>()",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ">".to_string(),
        },
    );
}

#[test]
fn test_error_function_trailing_comma_in_generics() {
    parser_error_test(
        "fn my_func<T,>()",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ">".to_string(),
        },
    );
}

#[test]
fn test_comment_between_function_name_and_params() {
    // This is unusual but should be syntactically valid.
    parser_test(
        "
fn my_func /* comment */ (a int)
    // body
",
        vec![func("my_func")
            .params(vec![parameter(
                "a".into(),
                type_expr_non_null(type_int()),
                None,
                None,
            )])
            .build_empty_body()],
    );
}

#[test]
fn test_async_function_declaration() {
    parser_test(
        "
async fn my_async_func()
    // body
",
        vec![func("my_async_func").set_async().build_empty_body()],
    );
}

#[test]
fn test_gpu_function_declaration() {
    parser_test(
        "
gpu fn my_gpu_func()
    // body
",
        vec![func("my_gpu_func").set_gpu().build_empty_body()],
    );
}

#[test]
fn test_async_gpu_function_declaration() {
    parser_error_test(
        "
async gpu fn my_async_gpu_func()
    // body
",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async gpu".to_string(),
            reason: "GPU kernels are inherently asynchronous.".to_string(),
        },
    );
}

#[test]
fn test_gpu_async_function_declaration_order() {
    // The order of modifiers should not matter for the error.
    parser_error_test(
        "
gpu async fn my_gpu_async_func()
    // body
",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async gpu".to_string(),
            reason: "GPU kernels are inherently asynchronous.".to_string(),
        },
    );
}

#[test]
fn test_parallel_function_declaration() {
    parser_test(
        "
parallel fn foo()
    // body
",
        vec![func("foo").set_parallel().build_empty_body()],
    );
}

#[test]
fn test_async_parallel_function_declaration() {
    parser_error_test(
        "
async parallel fn foo()
    // body
",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async parallel".to_string(),
            reason: "Parallel functions represent a different execution model and cannot be async."
                .to_string(),
        },
    );
}

#[test]
fn test_private_gpu_function_declaration() {
    parser_test(
        "
private gpu fn my_private_gpu_func()
    // body
",
        vec![func("my_private_gpu_func")
            .set_private()
            .set_gpu()
            .build_empty_body()],
    );
}

#[test]
fn test_all_modifiers_function_declaration() {
    parser_error_test(
        "
private async gpu fn my_uber_func()
    // body
",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async gpu".to_string(),
            reason: "GPU kernels are inherently asynchronous.".to_string(),
        },
    );
}

#[test]
fn test_error_modifier_after_func() {
    // Modifiers must precede the `def` keyword.
    parser_error_test(
        "fn gpu my_func()",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a function name, '(' or '<'".to_string(),
            found: "gpu".to_string(),
        },
    );
}

#[test]
fn test_protected_function() {
    parser_test(
        "protected fn my_func(): x",
        vec![func("my_func")
            .set_protected()
            .build(expression_statement(identifier("x")))],
    );
}

#[test]
fn test_private_async_gpu_function() {
    parser_error_test(
        "private async gpu fn complex_func(): x",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async gpu".to_string(),
            reason: "GPU kernels are inherently asynchronous.".to_string(),
        },
    );
}

#[test]
fn test_error_modifier_order_function() {
    // Visibility must come first. `async public` is invalid.
    parser_error_test(
        "async public fn my_func()",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "fn".to_string(),
            found: "public".to_string(),
        },
    );
}

#[test]
fn test_error_double_visibility_modifier() {
    parser_error_test(
        "public private fn my_func()",
        &SyntaxErrorKind::UnexpectedToken {
            expected:
                "let, var, const, async, fn, gpu, runtime, enum, type, struct or field declaration"
                    .to_string(),
            found: "private".to_string(),
        },
    );
}

#[test]
fn test_function_with_default_parameter_values() {
    parser_test(
        "
fn my_func(a int = 10, b bool = true)
    // body
",
        vec![func("my_func")
            .params(vec![
                parameter(
                    "a".into(),
                    type_expr_non_null(type_int()),
                    None,
                    opt_expr(int_literal_expression(10)),
                ),
                parameter(
                    "b".into(),
                    type_expr_non_null(type_bool()),
                    None,
                    opt_expr(boolean_literal(true)),
                ),
            ])
            .build_empty_body()],
    );
}

#[test]
fn test_function_and_parameter_names_as_keywords() {
    parser_error_test(
        "
fn if(let int, for String)
    // body
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a function name, '(' or '<'".to_string(),
            found: "if".to_string(),
        },
    );
}

#[test]
fn test_function_with_complex_generic_types() {
    parser_test(
        "
fn process<T>(data List<T>) List<T>: data
",
        vec![func("process")
            .generics(vec![generic_type("T", None)])
            .params(vec![parameter(
                "data".into(),
                type_expr_non_null(type_list(type_custom("T", None))),
                None,
                None,
            )])
            .return_type(type_expr_non_null(type_list(type_custom("T", None))))
            .build(expression_statement(identifier("data")))],
    );
}

#[test]
fn test_function_with_trailing_comma_in_parameters() {
    parser_test(
        "
fn my_func(a int, b String,)
    // body
",
        vec![func("my_func")
            .params(vec![
                parameter("a".into(), type_expr_non_null(type_int()), None, None),
                parameter("b".into(), type_expr_non_null(type_string()), None, None),
            ])
            .build_empty_body()],
    );
}

#[test]
fn test_toplevel_function_no_body_becomes_empty() {
    // A top-level function with no body (just newline) gets an empty statement body
    // This is the current parser behavior, but type checker should catch this
    parser_test(
        "
fn no_body()
",
        vec![func("no_body").build(empty_statement())],
    );
}

#[test]
fn test_toplevel_functions_with_blank_lines() {
    parser_test(
        "
fn a()
    1


fn b()
    2


fn c()
    3
",
        vec![
            func("a").build(block(vec![expression_statement(int_literal_expression(1))])),
            func("b").build(block(vec![expression_statement(int_literal_expression(2))])),
            func("c").build(block(vec![expression_statement(int_literal_expression(3))])),
        ],
    );
}

#[test]
fn test_toplevel_function_multiline_params() {
    // Function with parameters on multiple lines
    parser_test(
        "
fn create(
    name String,
    age int,
    active bool
)
    x
",
        vec![func("create")
            .params(vec![
                parameter("name".into(), type_expr_non_null(type_string()), None, None),
                parameter("age".into(), type_expr_non_null(type_int()), None, None),
                parameter("active".into(), type_expr_non_null(type_bool()), None, None),
            ])
            .build(block(vec![expression_statement(identifier("x"))]))],
    );
}

#[test]
fn test_function_implicit_return() {
    // Function with implicit return (last expression)
    parser_test(
        "
fn compute() int
    x + y
",
        vec![func("compute")
            .return_type(type_expr_non_null(type_int()))
            .build(block(vec![expression_statement(binary(
                identifier("x"),
                BinaryOp::Add,
                identifier("y"),
            ))]))],
    );
}

#[test]
fn test_function_with_guard_multiple_params() {
    // Multiple parameters with guards
    parser_test(
        "
fn divide(a int > 0, b int > 0) int
    a / b
",
        vec![func("divide")
            .params(vec![
                parameter(
                    "a".into(),
                    type_expr_non_null(type_int()),
                    opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                    None,
                ),
                parameter(
                    "b".into(),
                    type_expr_non_null(type_int()),
                    opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                    None,
                ),
            ])
            .return_type(type_expr_non_null(type_int()))
            .build(block(vec![expression_statement(binary(
                identifier("a"),
                BinaryOp::Div,
                identifier("b"),
            ))]))],
    );
}

#[test]
fn test_function_very_long_parameter_list() {
    // Function with many parameters
    parser_test(
        "
fn manyParams(a int, b int, c int, d int, e int)
    a
",
        vec![func("manyParams")
            .params(vec![
                parameter("a".into(), type_expr_non_null(type_int()), None, None),
                parameter("b".into(), type_expr_non_null(type_int()), None, None),
                parameter("c".into(), type_expr_non_null(type_int()), None, None),
                parameter("d".into(), type_expr_non_null(type_int()), None, None),
                parameter("e".into(), type_expr_non_null(type_int()), None, None),
            ])
            .build(block(vec![expression_statement(identifier("a"))]))],
    );
}
