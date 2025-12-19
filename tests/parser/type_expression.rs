// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_list_type_in_variable() {
    type_statement_test("[int]", typ(Type::List(Box::new(typ(Type::Int)))));
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
                null_typ(Type::Map(
                    Box::new(typ(Type::String)),
                    Box::new(typ(Type::Boolean)),
                )),
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
            .return_type(null_typ(Type::Tuple(vec![
                typ(Type::Float),
                null_typ(Type::Float),
                typ(Type::Float),
            ])))
            .build_empty_body()],
    );
}

#[test]
fn test_parse_generic_result_type() {
    type_statement_test(
        "result<int, string>",
        typ(Type::Result(
            Box::new(typ(Type::Int)),
            Box::new(typ(Type::String)),
        )),
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
            .return_type(typ(Type::Custom(
                "MyContainer".to_string(),
                Some(vec![
                    null_typ(Type::List(Box::new(typ(Type::Int)))), // [int]?
                    typ(Type::Future(Box::new(typ(Type::String)))), // future<string>
                ]),
            )))
            .build_empty_body()],
    );
}

#[test]
fn test_parse_set_type() {
    type_statement_test("{i64}", typ(Type::Set(Box::new(typ(Type::I64)))));
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
        null_typ(
            // The outer list is nullable: `[...]`?
            Type::List(Box::new(null_typ(
                // The inner list is nullable: `[{...}]?`
                Type::List(Box::new(typ(Type::Map(
                    // The map itself is not nullable
                    Box::new(typ(Type::String)),
                    Box::new(typ(Type::Tuple(vec![
                        null_typ(Type::Int), // int?
                        typ(Type::Boolean),
                    ]))),
                )))),
            ))),
        ),
    );
}

#[test]
fn test_grouping_parentheses_in_type() {
    type_statement_test("(string)", typ(Type::String));
}

#[test]
fn test_single_element_tuple_with_trailing_comma() {
    type_statement_test("(string,)", typ(Type::Tuple(vec![typ(Type::String)])));
}

#[test]
fn test_multi_element_tuple_with_trailing_comma() {
    type_statement_test(
        "(int, bool,)",
        typ(Type::Tuple(vec![typ(Type::Int), typ(Type::Boolean)])),
    );
}

#[test]
fn test_nullable_function_type() {
    type_statement_test(
        "(fn(s string) bool)?",
        null_typ(Type::Function(
            None,
            vec![parameter("s".into(), typ(Type::String), None, None)],
            opt_expr(typ(Type::Boolean)),
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
                opt_expr(typ(Type::Function(
                    None,
                    vec![],
                    opt_expr(null_typ(Type::Int)), // The `?` applies to the return type, not the function itself.
                ))),
                None,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_simple_nullable_built_in_type() {
    type_statement_test("int?", null_typ(Type::Int));
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
fn test_primitive_types() {
    let type_map = vec![
        ("int", Type::Int),
        ("i8", Type::I8),
        ("i16", Type::I16),
        ("i32", Type::I32),
        ("i64", Type::I64),
        ("i128", Type::I128),
        ("u8", Type::U8),
        ("u16", Type::U16),
        ("u32", Type::U32),
        ("u64", Type::U64),
        ("u128", Type::U128),
        ("float", Type::Float),
        ("f32", Type::F32),
        ("f64", Type::F64),
        ("string", Type::String),
        ("bool", Type::Boolean),
        ("symbol", Type::Symbol),
        (
            "result<int, string>",
            Type::Result(Box::new(typ(Type::Int)), Box::new(typ(Type::String))),
        ),
        ("list<float>", Type::List(Box::new(typ(Type::Float)))),
        (
            "map<string, int>",
            Type::Map(Box::new(typ(Type::String)), Box::new(typ(Type::Int))),
        ),
        ("set<string>", Type::Set(Box::new(typ(Type::String)))),
        ("future<string>", Type::Future(Box::new(typ(Type::String)))),
        (
            "tuple<string, int, float>",
            Type::Tuple(vec![typ(Type::String), typ(Type::Int), typ(Type::Float)]),
        ),
    ];

    for (name, mapped_type) in type_map {
        type_statement_test(name, typ(mapped_type.clone()));
        type_statement_test(format!("{}?", name).as_str(), null_typ(mapped_type.clone()));
    }
}

#[test]
fn test_function_type_with_named_parameters() {
    type_statement_test(
        "fn(x int, y string?) bool",
        typ(Type::Function(
            None, // no generics
            vec![
                parameter("x".into(), typ(Type::Int), None, None),
                parameter("y".into(), null_typ(Type::String), None, None),
            ],
            opt_expr(typ(Type::Boolean)),
        )),
    );
}

#[test]
fn test_function_type_returning_function_type() {
    // A function type that returns another function type.
    // Note: The inner function type must also have named parameters per current parser rules.
    type_statement_test(
        "fn() fn(x int) bool",
        typ(Type::Function(
            None,
            vec![],
            opt_expr(typ(Type::Function(
                None,
                vec![parameter("x".into(), typ(Type::Int), None, None)],
                opt_expr(typ(Type::Boolean)),
            ))),
        )),
    );
}

#[test]
fn test_list_of_function_types() {
    type_statement_test(
        "[fn(s string) bool]",
        typ(Type::List(Box::new(typ(Type::Function(
            None,
            vec![parameter("s".into(), typ(Type::String), None, None)],
            opt_expr(typ(Type::Boolean)),
        ))))),
    );
}

#[test]
fn test_function_type_with_generics() {
    type_statement_test(
        "fn<T>(item T) T",
        typ(Type::Function(
            Some(vec![generic_type("T", None)]),
            vec![parameter(
                "item".into(),
                typ(Type::Custom("T".into(), None)),
                None,
                None,
            )],
            opt_expr(typ(Type::Custom("T".into(), None))),
        )),
    );
}

#[test]
fn test_crazy_nested_function_type() {
    type_statement_test(
        "fn<T>(cb fn(item T) bool, items [T]?) (fn() T)?",
        typ(Type::Function(
            Some(vec![generic_type("T", None)]), // generics: <T>
            vec![
                // parameters
                parameter(
                    "cb".into(),
                    typ(Type::Function(
                        None,
                        vec![parameter(
                            "item".into(),
                            typ(Type::Custom("T".into(), None)),
                            None,
                            None,
                        )],
                        opt_expr(typ(Type::Boolean)),
                    )),
                    None,
                    None,
                ),
                parameter(
                    "items".into(),
                    null_typ(Type::List(Box::new(typ(Type::Custom("T".into(), None))))),
                    None,
                    None,
                ),
            ],
            // return type: (fn() T)?
            opt_expr(null_typ(Type::Function(
                // <-- This is now a nullable function, not a tuple
                None,
                vec![],
                opt_expr(typ(Type::Custom("T".into(), None))),
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
                opt_expr(typ(Type::Function(
                    None,
                    vec![parameter("a".into(), typ(Type::Int), None, None)],
                    None,
                ))),
                None,
            )],
            MemberVisibility::Public,
        )],
    );
}
