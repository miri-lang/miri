// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_function_declaration() {
    parser_test(
        "
fn square(x int)
    x * x
",
        vec![func("square")
            .params(vec![parameter("x".into(), typ(Type::Int), None, None)])
            .build(block(vec![expression_statement(binary(
                identifier("x".into()),
                BinaryOp::Mul,
                identifier("x".into()),
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
                typ(Type::Int),
                opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                None,
            )])
            .build(block(vec![expression_statement(binary(
                identifier("x".into()),
                BinaryOp::Mul,
                identifier("x".into()),
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
                typ(Type::Int),
                opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                None,
            )])
            .return_type(typ(Type::Int))
            .build(expression_statement(binary(
                identifier("x".into()),
                BinaryOp::Mul,
                identifier("x".into()),
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
            .return_type(typ(Type::Int))
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
                parameter("a".into(), typ(Type::Int), None, None),
                parameter("b".into(), typ(Type::Int), None, None),
            ])
            .build(block(vec![return_statement(opt_expr(binary(
                identifier("a".into()),
                BinaryOp::Add,
                identifier("b".into()),
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
                opt_expr(typ(Type::Custom("SomeClass".into(), None))),
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
                    opt_expr(typ(Type::Custom("SomeTrait".into(), None))),
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
                typ(Type::Custom("T".into(), None)),
                None,
                None,
            )])
            .return_type(typ(Type::Custom("T".into(), None)))
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
            .params(vec![parameter("a".into(), typ(Type::Int), None, None)])
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
    parser_test(
        "
async gpu fn my_async_gpu_func()
    // body
",
        vec![func("my_async_gpu_func")
            .set_async()
            .set_gpu()
            .build_empty_body()],
    );
}

#[test]
fn test_gpu_async_function_declaration_order() {
    // The order of modifiers should not matter.
    parser_test(
        "
gpu async fn my_gpu_async_func()
    // body
",
        vec![func("my_gpu_async_func")
            .set_gpu()
            .set_async()
            .build_empty_body()],
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
    parser_test(
        "
private async gpu fn my_uber_func()
    // body
",
        vec![func("my_uber_func")
            .set_private()
            .set_async()
            .set_gpu()
            .build_empty_body()],
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
    parser_test(
        "private async gpu fn complex_func(): x",
        vec![func("complex_func")
            .set_private()
            .set_async()
            .set_gpu()
            .build(expression_statement(identifier("x")))],
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
            expected: "let, var, async, def, gpu, enum, type or struct".to_string(),
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
                    typ(Type::Int),
                    None,
                    opt_expr(int_literal_expression(10)),
                ),
                parameter(
                    "b".into(),
                    typ(Type::Boolean),
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
fn if(let int, for string)
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
fn process<T>(data list<T>) list<T>: data
",
        vec![func("process")
            .generics(vec![generic_type("T", None)])
            .params(vec![parameter(
                "data".into(),
                typ(Type::List(Box::new(typ(Type::Custom("T".into(), None))))),
                None,
                None,
            )])
            .return_type(typ(Type::List(Box::new(typ(Type::Custom(
                "T".into(),
                None,
            ))))))
            .build(expression_statement(identifier("data")))],
    );
}

#[test]
fn test_function_with_trailing_comma_in_parameters() {
    parser_test(
        "
fn my_func(a int, b string,)
    // body
",
        vec![func("my_func")
            .params(vec![
                parameter("a".into(), typ(Type::Int), None, None),
                parameter("b".into(), typ(Type::String), None, None),
            ])
            .build_empty_body()],
    );
}
