// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{
    parser_error_test, parser_test, type_list_expr, type_map_expr, type_statement_test,
    type_tuple_expr,
};
use miri::ast::factory::{
    func, generic_type, int_literal_expression, let_variable, parameter, type_array, type_bool,
    type_custom, type_expr_non_null, type_expr_null, type_f32, type_f64, type_float, type_function,
    type_future, type_i128, type_i16, type_i32, type_i64, type_i8, type_int, type_list, type_map,
    type_result, type_set, type_string, type_symbol, type_tuple, type_u128, type_u16, type_u32,
    type_u64, type_u8, variable_statement,
};
use miri::ast::{opt_expr, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_list_type_in_variable() {
    type_statement_test("[int]", type_expr_non_null(type_list(type_int())));
}

#[test]
fn test_fixed_size_array_type() {
    type_statement_test(
        "[int; 3]",
        type_expr_non_null(type_array(type_int(), Box::new(int_literal_expression(3)))),
    );
}

#[test]
fn test_parse_nullable_map_type_in_parameter() {
    parser_test(
        "
fn process_data(data {string: bool}?)
    // body
",
        vec![func("process_data")
            .params(vec![parameter(
                "data".into(),
                type_expr_null(type_map(type_string(), type_bool())),
                None,
                None,
            )])
            .build_empty_body()],
    );
}

#[test]
fn test_parse_tuple_type_as_return_type() {
    parser_test(
        "
fn get_coordinates() (float, float?, float)?
    // body
",
        vec![func("get_coordinates")
            .return_type(type_expr_null(type_tuple_expr(vec![
                type_expr_non_null(type_float()),
                type_expr_null(type_float()),
                type_expr_non_null(type_float()),
            ])))
            .build_empty_body()],
    );
}

#[test]
fn test_parse_generic_result_type() {
    type_statement_test(
        "result<int, string>",
        type_expr_non_null(type_result(type_int(), type_string())),
    );
}

#[test]
fn test_parse_generic_custom_type_with_nesting() {
    parser_test(
        "
fn get_data() MyContainer<[int]?, future<string>>
    // body
",
        vec![func("get_data")
            .return_type(type_expr_non_null(type_custom(
                "MyContainer",
                Some(vec![
                    type_expr_null(type_list(type_int())),          // [int]?
                    type_expr_non_null(type_future(type_string())), // future<string>
                ]),
            )))
            .build_empty_body()],
    );
}

#[test]
fn test_parse_set_type() {
    type_statement_test("{i64}", type_expr_non_null(type_set(type_i64())));
}

#[test]
fn test_error_unclosed_list_type() {
    parser_error_test("let my_list [int", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_malformed_map_type() {
    parser_error_test(
        "let my_map {string, int}",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "}".to_string(),
            found: ",".to_string(),
        },
    );
}

#[test]
fn test_error_incomplete_generic_type() {
    parser_error_test(
        "let my_generic MyType<int,",
        &SyntaxErrorKind::UnexpectedEOF,
    );
}

#[test]
fn test_error_empty_generic_parameters() {
    parser_error_test(
        "let my_generic MyType<>",
        &SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Generic type".to_string(),
        },
    );
}

#[test]
fn test_deeply_nested_collection_type() {
    type_statement_test(
        "[[{string: (int?, bool)}]?]?",
        type_expr_null(
            // The outer list is nullable: `[...]`?
            type_list_expr(type_expr_null(
                // The inner list is nullable: `[{...}]?`
                type_list_expr(type_expr_non_null(type_map_expr(
                    // The map itself is not nullable
                    type_expr_non_null(type_string()),
                    type_expr_non_null(type_tuple_expr(vec![
                        type_expr_null(type_int()), // int?
                        type_expr_non_null(type_bool()),
                    ])),
                ))),
            )),
        ),
    );
}

#[test]
fn test_grouping_parentheses_in_type() {
    type_statement_test("(string)", type_expr_non_null(type_string()));
}

#[test]
fn test_single_element_tuple_with_trailing_comma() {
    type_statement_test(
        "(string,)",
        type_expr_non_null(type_tuple(vec![type_string()])),
    );
}

#[test]
fn test_multi_element_tuple_with_trailing_comma() {
    type_statement_test(
        "(int, bool,)",
        type_expr_non_null(type_tuple(vec![type_int(), type_bool()])),
    );
}

#[test]
fn test_nullable_function_type() {
    type_statement_test(
        "(fn(s string) bool)?",
        type_expr_null(type_function(
            None,
            vec![parameter(
                "s".into(),
                type_expr_non_null(type_string()),
                None,
                None,
            )],
            opt_expr(type_expr_non_null(type_bool())),
        )),
    );
}

#[test]
fn test_error_ambiguous_nullable_function_return() {
    parser_test(
        "let x fn() int?",
        vec![variable_statement(
            vec![let_variable(
                "x",
                opt_expr(type_expr_non_null(type_function(
                    None,
                    vec![],
                    opt_expr(type_expr_null(type_int())), // The `?` applies to the return type, not the function itself.
                ))),
                None,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_simple_nullable_built_in_type() {
    type_statement_test("int?", type_expr_null(type_int()));
}

#[test]
fn test_error_map_missing_value_type() {
    parser_error_test(
        "let x {string:}",
        &SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Map value type".to_string(),
        },
    );
}

#[test]
fn test_error_result_type_missing_parameter() {
    parser_error_test(
        "let x result<int>",
        &SyntaxErrorKind::UnexpectedToken {
            expected: ",".to_string(),
            found: ">".to_string(),
        },
    );
}

#[test]
fn test_error_double_nullable() {
    parser_error_test(
        "let x int??",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "?".to_string(),
        },
    );
}

#[test]
fn test_system_types() {
    let type_map = vec![
        ("int", type_int()),
        ("i8", type_i8()),
        ("i16", type_i16()),
        ("i32", type_i32()),
        ("i64", type_i64()),
        ("i128", type_i128()),
        ("u8", type_u8()),
        ("u16", type_u16()),
        ("u32", type_u32()),
        ("u64", type_u64()),
        ("u128", type_u128()),
        ("float", type_float()),
        ("f32", type_f32()),
        ("f64", type_f64()),
        ("string", type_string()),
        ("bool", type_bool()),
        ("symbol", type_symbol()),
        (
            "result<int, string>",
            type_result(type_int(), type_string()),
        ),
        ("list<float>", type_list(type_float())),
        ("map<string, int>", type_map(type_string(), type_int())),
        ("set<string>", type_set(type_string())),
        ("future<string>", type_future(type_string())),
        (
            "tuple<string, int, float>",
            type_tuple(vec![type_string(), type_int(), type_float()]),
        ),
        (
            "array<string, 3>",
            type_array(type_string(), Box::new(int_literal_expression(3))),
        ),
    ];

    for (name, mapped_type) in type_map {
        type_statement_test(name, type_expr_non_null(mapped_type.clone()));
        type_statement_test(
            format!("{}?", name).as_str(),
            type_expr_null(mapped_type.clone()),
        );
    }
}

#[test]
fn test_function_type_with_named_parameters() {
    type_statement_test(
        "fn(x int, y string?) bool",
        type_expr_non_null(type_function(
            None, // no generics
            vec![
                parameter("x".into(), type_expr_non_null(type_int()), None, None),
                parameter("y".into(), type_expr_null(type_string()), None, None),
            ],
            opt_expr(type_expr_non_null(type_bool())),
        )),
    );
}

#[test]
fn test_function_type_returning_function_type() {
    // A function type that returns another function type.
    // Note: The inner function type must also have named parameters per current parser rules.
    type_statement_test(
        "fn() fn(x int) bool",
        type_expr_non_null(type_function(
            None,
            vec![],
            opt_expr(type_expr_non_null(type_function(
                None,
                vec![parameter(
                    "x".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                opt_expr(type_expr_non_null(type_bool())),
            ))),
        )),
    );
}

#[test]
fn test_list_of_function_types() {
    type_statement_test(
        "[fn(s string) bool]",
        type_expr_non_null(type_list(type_function(
            None,
            vec![parameter(
                "s".into(),
                type_expr_non_null(type_string()),
                None,
                None,
            )],
            opt_expr(type_expr_non_null(type_bool())),
        ))),
    );
}

#[test]
fn test_function_type_with_generics() {
    type_statement_test(
        "fn<T>(item T) T",
        type_expr_non_null(type_function(
            Some(vec![generic_type("T", None)]),
            vec![parameter(
                "item".into(),
                type_expr_non_null(type_custom("T", None)),
                None,
                None,
            )],
            opt_expr(type_expr_non_null(type_custom("T", None))),
        )),
    );
}

#[test]
fn test_crazy_nested_function_type() {
    type_statement_test(
        "fn<T>(cb fn(item T) bool, items [T]?) (fn() T)?",
        type_expr_non_null(type_function(
            Some(vec![generic_type("T", None)]), // generics: <T>
            vec![
                // parameters
                parameter(
                    "cb".into(),
                    type_expr_non_null(type_function(
                        None,
                        vec![parameter(
                            "item".into(),
                            type_expr_non_null(type_custom("T", None)),
                            None,
                            None,
                        )],
                        opt_expr(type_expr_non_null(type_bool())),
                    )),
                    None,
                    None,
                ),
                parameter(
                    "items".into(),
                    type_expr_null(type_list(type_custom("T", None))),
                    None,
                    None,
                ),
            ],
            // return type: (fn() T)?
            opt_expr(type_expr_null(type_function(
                // <-- This is now a nullable function, not a tuple
                None,
                vec![],
                opt_expr(type_expr_non_null(type_custom("T", None))),
            ))),
        )),
    );
}

#[test]
fn test_function_type_with_trailing_comma() {
    parser_test(
        "let x fn(a int,)",
        vec![variable_statement(
            vec![let_variable(
                "x",
                opt_expr(type_expr_non_null(type_function(
                    None,
                    vec![parameter(
                        "a".into(),
                        type_expr_non_null(type_int()),
                        None,
                        None,
                    )],
                    None,
                ))),
                None,
            )],
            MemberVisibility::Public,
        )],
    );
}
