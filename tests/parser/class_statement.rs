// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{
    abstract_class_statement_test, class_statement_test, parser_error_test, parser_test,
};
use miri::ast::factory::{
    abstract_function_declaration, assign, binary, block, call, class_statement, empty_statement,
    expression_statement, function_declaration, generic_type, generic_type_with_kind, identifier,
    let_variable, lhs_member, member, parameter, string_literal_expression, super_expression,
    type_custom, type_expr_non_null, type_float, type_int, type_list, type_string, var,
    variable_statement,
};
use miri::ast::types::TypeDeclarationKind;
use miri::ast::{opt_expr, AssignmentOp, BinaryOp, FunctionProperties, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_empty_class() {
    parser_test(
        "
class MyClass
    let x int
",
        vec![class_statement(
            identifier("MyClass"),
            None,
            None,
            vec![],
            vec![variable_statement(
                vec![let_variable(
                    "x",
                    opt_expr(type_expr_non_null(type_int())),
                    None,
                )],
                MemberVisibility::Private,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_class_with_field() {
    parser_test(
        "
class Point
    let x int
    let y int
",
        vec![class_statement(
            identifier("Point"),
            None,
            None,
            vec![],
            vec![
                variable_statement(
                    vec![let_variable(
                        "x",
                        opt_expr(type_expr_non_null(type_int())),
                        None,
                    )],
                    MemberVisibility::Private,
                ),
                variable_statement(
                    vec![let_variable(
                        "y",
                        opt_expr(type_expr_non_null(type_int())),
                        None,
                    )],
                    MemberVisibility::Private,
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_class_with_mutable_field() {
    parser_test(
        "
class Counter
    var count int
",
        vec![class_statement(
            identifier("Counter"),
            None,
            None,
            vec![],
            vec![variable_statement(
                vec![var("count", opt_expr(type_expr_non_null(type_int())), None)],
                MemberVisibility::Private,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_class_with_method() {
    class_statement_test(
        "
class Calculator
    fn add(a int, b int) int
        a + b
",
        identifier("Calculator"),
        None,
        None,
        vec![],
        vec![function_declaration(
            "add",
            None,
            vec![
                parameter("a".into(), type_expr_non_null(type_int()), None, None),
                parameter("b".into(), type_expr_non_null(type_int()), None, None),
            ],
            Some(Box::new(type_expr_non_null(type_int()))),
            block(vec![expression_statement(binary(
                identifier("a"),
                BinaryOp::Add,
                identifier("b"),
            ))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_with_init() {
    class_statement_test(
        "
class Point
    let x int
    fn init(x int)
        self.x = x
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![
            variable_statement(
                vec![let_variable(
                    "x",
                    opt_expr(type_expr_non_null(type_int())),
                    None,
                )],
                MemberVisibility::Private,
            ),
            function_declaration(
                "init",
                None,
                vec![parameter(
                    "x".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                None,
                block(vec![expression_statement(assign(
                    lhs_member(identifier("self"), identifier("x")),
                    AssignmentOp::Assign,
                    identifier("x"),
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
fn test_class_extends() {
    class_statement_test(
        "
class Dog extends Animal
    let name string
",
        identifier("Dog"),
        None,
        Some(Box::new(identifier("Animal"))),
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "name",
                opt_expr(type_expr_non_null(type_string())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_implements() {
    class_statement_test(
        "
class MyList implements Iterable
    let items int
",
        identifier("MyList"),
        None,
        None,
        vec![identifier("Iterable")],
        vec![variable_statement(
            vec![let_variable(
                "items",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_implements_multiple() {
    class_statement_test(
        "
class MyList implements Iterable, Sortable
    let items int
",
        identifier("MyList"),
        None,
        None,
        vec![identifier("Iterable"), identifier("Sortable")],
        vec![variable_statement(
            vec![let_variable(
                "items",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_extends_and_implements() {
    class_statement_test(
        "
class Dog extends Animal implements Trainable
    let name string
",
        identifier("Dog"),
        None,
        Some(Box::new(identifier("Animal"))),
        vec![identifier("Trainable")],
        vec![variable_statement(
            vec![let_variable(
                "name",
                opt_expr(type_expr_non_null(type_string())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_public_field() {
    class_statement_test(
        "
class Point
    public let x int
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "x",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Public,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_protected_method() {
    class_statement_test(
        "
class Point
    protected fn helper()
        x
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![function_declaration(
            "helper",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Protected,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_private_field_explicit() {
    class_statement_test(
        "
class Point
    private let x int
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "x",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_with_generics() {
    class_statement_test(
        "
class Box<T>
    let value T
",
        identifier("Box"),
        Some(vec![generic_type("T", None)]),
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "value",
                opt_expr(type_expr_non_null(type_custom("T", None))),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_with_generic_constraint() {
    class_statement_test(
        "
class SortedList<T extends Comparable>
    let items T
",
        identifier("SortedList"),
        Some(vec![generic_type_with_kind(
            "T",
            Some(Box::new(type_expr_non_null(type_custom(
                "Comparable",
                None,
            )))),
            TypeDeclarationKind::Extends,
        )]),
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "items",
                opt_expr(type_expr_non_null(type_custom("T", None))),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_super_expression() {
    parser_test(
        "super.init()",
        vec![expression_statement(call(
            member(super_expression(), identifier("init")),
            vec![],
        ))],
    );
}

#[test]
fn test_super_method_with_args() {
    parser_test(
        "super.method(x, y)",
        vec![expression_statement(call(
            member(super_expression(), identifier("method")),
            vec![identifier("x"), identifier("y")],
        ))],
    );
}

#[test]
fn test_error_class_no_body() {
    // This should actually succeed with empty body according to our implementation
    // But if we require at least one member, uncomment below
    // parser_error_test("class Empty", &SyntaxErrorKind::...);
}

#[test]
fn test_error_class_implements_before_extends() {
    parser_error_test(
        "
class Dog implements Trainable extends Animal
    let x int
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "extends".to_string(),
        },
    );
}

#[test]
fn test_error_class_invalid_member() {
    parser_error_test(
        "
class Point
    if x
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "class member (let, var, fn, async, gpu, or type)".to_string(),
            found: "if".to_string(),
        },
    );
}

// Abstract Classes

#[test]
fn test_abstract_class_basic() {
    // Basic abstract class
    abstract_class_statement_test(
        "
abstract class Shape
    let name string
",
        identifier("Shape"),
        None,
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "name",
                opt_expr(type_expr_non_null(type_string())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_abstract_class_with_abstract_method() {
    // Abstract class with abstract method (no body)
    abstract_class_statement_test(
        "
abstract class Shape
    fn area() float
",
        identifier("Shape"),
        None,
        None,
        vec![],
        vec![abstract_function_declaration(
            "area",
            None,
            vec![],
            Some(Box::new(type_expr_non_null(type_float()))),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_abstract_class_with_concrete_method() {
    // Abstract class with concrete method (has body)
    abstract_class_statement_test(
        "
abstract class Shape
    fn describe() string
        \"A shape\"
",
        identifier("Shape"),
        None,
        None,
        vec![],
        vec![function_declaration(
            "describe",
            None,
            vec![],
            Some(Box::new(type_expr_non_null(type_string()))),
            block(vec![expression_statement(string_literal_expression(
                "A shape",
            ))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_abstract_class_mixed_members() {
    // Abstract class with fields and mixed methods
    abstract_class_statement_test(
        "
abstract class Animal
    let name string
    var age int
    fn speak() string
    fn sleep()
        x
",
        identifier("Animal"),
        None,
        None,
        vec![],
        vec![
            variable_statement(
                vec![let_variable(
                    "name",
                    opt_expr(type_expr_non_null(type_string())),
                    None,
                )],
                MemberVisibility::Private,
            ),
            variable_statement(
                vec![var("age", opt_expr(type_expr_non_null(type_int())), None)],
                MemberVisibility::Private,
            ),
            abstract_function_declaration(
                "speak",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_string()))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            function_declaration(
                "sleep",
                None,
                vec![],
                None,
                block(vec![expression_statement(identifier("x"))]),
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
fn test_abstract_class_extends() {
    // Abstract class extending another class
    abstract_class_statement_test(
        "
abstract class Dog extends Animal
    fn bark() string
",
        identifier("Dog"),
        None,
        Some(Box::new(identifier("Animal"))),
        vec![],
        vec![abstract_function_declaration(
            "bark",
            None,
            vec![],
            Some(Box::new(type_expr_non_null(type_string()))),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_abstract_class_implements() {
    // Abstract class implementing a trait
    abstract_class_statement_test(
        "
abstract class Handler implements Runnable
    fn handle()
",
        identifier("Handler"),
        None,
        None,
        vec![identifier("Runnable")],
        vec![abstract_function_declaration(
            "handle",
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
fn test_abstract_class_extends_and_implements() {
    // Abstract class extending and implementing
    abstract_class_statement_test(
        "
abstract class Service extends BaseService implements Callable, Serializable
    fn execute()
    fn serialize() string
",
        identifier("Service"),
        None,
        Some(Box::new(identifier("BaseService"))),
        vec![identifier("Callable"), identifier("Serializable")],
        vec![
            abstract_function_declaration(
                "execute",
                None,
                vec![],
                None,
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
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
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_abstract_class_with_generics() {
    // Abstract class with generic type
    abstract_class_statement_test(
        "
abstract class Container<T>
    fn get() T
    fn set(value T)
",
        identifier("Container"),
        Some(vec![generic_type("T", None)]),
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "get",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_custom("T", None)))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "set",
                None,
                vec![parameter(
                    "value".into(),
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
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_abstract_class_with_constrained_generics() {
    // Abstract class with constrained generic type
    abstract_class_statement_test(
        "
abstract class SortedContainer<T extends Comparable>
    fn add(item T)
    fn sort()
",
        identifier("SortedContainer"),
        Some(vec![generic_type_with_kind(
            "T",
            Some(Box::new(type_expr_non_null(type_custom(
                "Comparable",
                None,
            )))),
            TypeDeclarationKind::Extends,
        )]),
        None,
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
                "sort",
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
fn test_abstract_class_multiple_abstract_methods() {
    // Abstract class with multiple abstract methods
    abstract_class_statement_test(
        "
abstract class Repository
    fn findAll() list<int>
    fn findById(id int) int
    fn save(entity int)
    fn delete(id int)
",
        identifier("Repository"),
        None,
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "findAll",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_list(type_int())))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "findById",
                None,
                vec![parameter(
                    "id".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_int()))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "save",
                None,
                vec![parameter(
                    "entity".into(),
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
fn test_abstract_class_with_visibility_modifiers() {
    // Abstract class with visibility modifiers on members
    abstract_class_statement_test(
        "
abstract class Base
    public fn publicMethod()
    protected fn protectedMethod()
    private fn privateMethod()
        x
",
        identifier("Base"),
        None,
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
            function_declaration(
                "privateMethod",
                None,
                vec![],
                None,
                block(vec![expression_statement(identifier("x"))]),
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
fn test_abstract_class_async_method() {
    // Abstract class with async method
    abstract_class_statement_test(
        "
abstract class AsyncHandler
    async fn handle()
",
        identifier("AsyncHandler"),
        None,
        None,
        vec![],
        vec![abstract_function_declaration(
            "handle",
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

#[test]
fn test_class_function_without_body() {
    // NOTE: The parser allows empty function bodies in classes,
    // but the type checker will reject them.
    class_statement_test(
        "
class Regular
    fn compute() int
",
        identifier("Regular"),
        None,
        None,
        vec![],
        vec![function_declaration(
            "compute",
            None,
            vec![],
            Some(Box::new(type_expr_non_null(type_int()))),
            empty_statement(),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_function_no_indent() {
    // Without indentation, the functions are parsed as top-level functions,
    // and the class is parsed as an empty class. This is valid syntax.
    parser_test(
        "
class Regular
fn compute() int
fn calc() int
",
        vec![
            // Empty class
            class_statement(
                identifier("Regular"),
                None,
                None,
                vec![],
                vec![],
                MemberVisibility::Public,
            ),
            // Top-level function 'compute'
            function_declaration(
                "compute",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_int()))),
                empty_statement(),
                FunctionProperties {
                    // Top-level functions default to Public
                    visibility: MemberVisibility::Public,
                    ..Default::default()
                },
            ),
            // Top-level function 'calc'
            function_declaration(
                "calc",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_int()))),
                empty_statement(),
                FunctionProperties {
                    visibility: MemberVisibility::Public,
                    ..Default::default()
                },
            ),
        ],
    );
}

#[test]
fn test_abstractclass_functions_with_weird_indent() {
    abstract_class_statement_test(
        "
abstract class Regular
// Comment
    fn compute() int
/*
  Some other comment

*/

    // Comment 2
    fn calc() int
",
        identifier("Regular"),
        None,
        None,
        vec![],
        vec![
            abstract_function_declaration(
                "compute",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_int()))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            abstract_function_declaration(
                "calc",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_int()))),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}
