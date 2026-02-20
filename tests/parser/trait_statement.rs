// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, trait_statement_test};
use miri::ast::factory::{
    abstract_function_declaration, block, boolean_literal, expression_statement,
    function_declaration, generic_type, generic_type_with_kind, identifier, int_literal_expression,
    parameter, string_literal_expression, type_bool, type_custom, type_expr_non_null, type_int,
    type_string,
};
use miri::ast::types::TypeDeclarationKind;
use miri::ast::{opt_expr, FunctionProperties, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_simple_trait() {
    trait_statement_test(
        "
trait Drawable
    fn draw()
        x
",
        identifier("Drawable"),
        None,
        vec![],
        vec![function_declaration(
            "draw",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_with_multiple_methods() {
    trait_statement_test(
        "
trait Comparable
    fn compare(other int) int
        0
    fn equals(other int) bool
        true
",
        identifier("Comparable"),
        None,
        vec![],
        vec![
            function_declaration(
                "compare",
                None,
                vec![parameter(
                    "other".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_int()))),
                block(vec![expression_statement(int_literal_expression(0))]),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            function_declaration(
                "equals",
                None,
                vec![parameter(
                    "other".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_bool()))),
                block(vec![expression_statement(boolean_literal(true))]),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_extends_single() {
    trait_statement_test(
        "
trait Sortable extends Comparable
    fn sort()
        x
",
        identifier("Sortable"),
        None,
        vec![identifier("Comparable")],
        vec![function_declaration(
            "sort",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_extends_multiple() {
    trait_statement_test(
        "
trait ReadWrite extends Readable, Writable
    fn readwrite()
        x
",
        identifier("ReadWrite"),
        None,
        vec![identifier("Readable"), identifier("Writable")],
        vec![function_declaration(
            "readwrite",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_with_generics() {
    trait_statement_test(
        "
trait Container<T>
    fn add(item T)
        x
",
        identifier("Container"),
        Some(vec![generic_type("T", None)]),
        vec![],
        vec![function_declaration(
            "add",
            None,
            vec![parameter(
                "item".into(),
                type_expr_non_null(type_custom("T", None)),
                None,
                None,
            )],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_with_generic_constraint() {
    trait_statement_test(
        "
trait OrderedContainer<T extends Comparable>
    fn sort()
        x
",
        identifier("OrderedContainer"),
        Some(vec![generic_type_with_kind(
            "T",
            Some(Box::new(type_expr_non_null(type_custom(
                "Comparable",
                None,
            )))),
            TypeDeclarationKind::Extends,
        )]),
        vec![],
        vec![function_declaration(
            "sort",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_public_trait() {
    trait_statement_test(
        "
trait API
    fn call()
        x
",
        identifier("API"),
        None,
        vec![],
        vec![function_declaration(
            "call",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_error_trait_with_implements() {
    // Traits use extends for inheritance, not implements
    parser_error_test(
        "
trait Invalid implements Something
    fn method()
        x
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "implements".to_string(),
        },
    );
}

#[test]
fn test_error_trait_invalid_member() {
    parser_error_test(
        "
trait Invalid
    for x in y",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "class member (let, var, const, fn, async, gpu, type, or field declaration)"
                .to_string(),
            found: "for".to_string(),
        },
    );
}

// ==========================================
// Abstract Functions in Traits
// ==========================================

#[test]
fn test_trait_abstract_function_no_body() {
    // Abstract function: just a signature, no body
    trait_statement_test(
        "
trait Drawable
    fn draw()
",
        identifier("Drawable"),
        None,
        vec![],
        vec![abstract_function_declaration(
            "draw",
            None,
            vec![],
            None,
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_abstract_function_with_return_type() {
    // Abstract function with return type
    trait_statement_test(
        "
trait Measurable
    fn size() int
",
        identifier("Measurable"),
        None,
        vec![],
        vec![abstract_function_declaration(
            "size",
            None,
            vec![],
            Some(Box::new(type_expr_non_null(type_int()))),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_abstract_function_with_parameters() {
    // Abstract function with parameters
    trait_statement_test(
        "
trait Processor
    fn process(data String, count int) bool
",
        identifier("Processor"),
        None,
        vec![],
        vec![abstract_function_declaration(
            "process",
            None,
            vec![
                parameter("data".into(), type_expr_non_null(type_string()), None, None),
                parameter("count".into(), type_expr_non_null(type_int()), None, None),
            ],
            Some(Box::new(type_expr_non_null(type_bool()))),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_multiple_abstract_functions() {
    // Multiple abstract functions
    trait_statement_test(
        "
trait CRUD
    fn create(data String)
    fn read(id int) String
    fn update(id int, data String)
    fn delete(id int)
",
        identifier("CRUD"),
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "create",
                None,
                vec![parameter(
                    "data".into(),
                    type_expr_non_null(type_string()),
                    None,
                    None,
                )],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "read",
                None,
                vec![parameter(
                    "id".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_string()))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "update",
                None,
                vec![
                    parameter("id".into(), type_expr_non_null(type_int()), None, None),
                    parameter("data".into(), type_expr_non_null(type_string()), None, None),
                ],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "delete",
                None,
                vec![parameter(
                    "id".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_mixed_abstract_and_concrete_functions() {
    // Mix of abstract (no body) and concrete (with body) functions
    trait_statement_test(
        "
trait Serializable
    fn serialize() String
    fn deserialize(data String)
    fn toString() String
        \"default\"
",
        identifier("Serializable"),
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "serialize",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_string()))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "deserialize",
                None,
                vec![parameter(
                    "data".into(),
                    type_expr_non_null(type_string()),
                    None,
                    None,
                )],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            function_declaration(
                "toString",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_string()))),
                block(vec![expression_statement(string_literal_expression(
                    "default",
                ))]),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_abstract_function_with_generics() {
    // Trait with generic type and abstract function using it
    trait_statement_test(
        "
trait Container<T>
    fn add(item T)
    fn get(index int) T
",
        identifier("Container"),
        Some(vec![generic_type("T", None)]),
        vec![],
        vec![
            abstract_function_declaration(
                "add",
                None,
                vec![parameter(
                    "item".into(),
                    type_expr_non_null(type_custom("T", None)),
                    None,
                    None,
                )],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "get",
                None,
                vec![parameter(
                    "index".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_custom("T", None)))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_abstract_with_visibility_modifiers() {
    // Abstract functions with visibility modifiers
    trait_statement_test(
        "
trait API
    public fn publicMethod()
    protected fn protectedMethod()
    private fn privateMethod()
",
        identifier("API"),
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "publicMethod",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Public,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "protectedMethod",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Protected,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "privateMethod",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_abstract_function_compact_format() {
    // Compact: multiple functions on consecutive lines, no extra blank lines
    trait_statement_test(
        "
trait Compact
    fn a()
    fn b()
    fn c()
",
        identifier("Compact"),
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "a",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "b",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "c",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_extends_with_abstract_functions() {
    // Trait extends another with abstract functions
    trait_statement_test(
        "
trait Child extends Parent
    fn childMethod()
",
        identifier("Child"),
        None,
        vec![identifier("Parent")],
        vec![abstract_function_declaration(
            "childMethod",
            None,
            vec![],
            None,
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_extends_multiple_with_abstract_functions() {
    // Trait extends multiple traits with abstract functions
    trait_statement_test(
        "
trait Combined extends Readable, Writable
    fn combined()
",
        identifier("Combined"),
        None,
        vec![identifier("Readable"), identifier("Writable")],
        vec![abstract_function_declaration(
            "combined",
            None,
            vec![],
            None,
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_abstract_function_with_default_params() {
    // Abstract function with default parameter values
    trait_statement_test(
        "
trait Configurable
    fn configure(timeout int = 30, retries int = 3)
",
        identifier("Configurable"),
        None,
        vec![],
        vec![abstract_function_declaration(
            "configure",
            None,
            vec![
                parameter(
                    "timeout".into(),
                    type_expr_non_null(type_int()),
                    None,
                    opt_expr(int_literal_expression(30)),
                ),
                parameter(
                    "retries".into(),
                    type_expr_non_null(type_int()),
                    None,
                    opt_expr(int_literal_expression(3)),
                ),
            ],
            None,
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_async_abstract_function() {
    // Async abstract function
    trait_statement_test(
        "
trait AsyncProcessor
    async fn processAsync()
",
        identifier("AsyncProcessor"),
        None,
        vec![],
        vec![abstract_function_declaration(
            "processAsync",
            None,
            vec![],
            None,
            FunctionProperties {
                visibility: MemberVisibility::Private,
                is_async: true,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}
