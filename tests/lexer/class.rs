// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::lexer_token_test;

#[test]
fn test_class_keyword() {
    lexer_token_test("class", vec![Token::Class]);
}

#[test]
fn test_trait_keyword() {
    lexer_token_test("trait", vec![Token::Trait]);
}

#[test]
fn test_super_keyword() {
    lexer_token_test("super", vec![Token::Super]);
}

#[test]
fn test_class_in_contexts() {
    lexer_token_test(
        "class class_name classname name_class",
        vec![
            Token::Class,
            Token::Identifier,
            Token::Identifier,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_trait_in_contexts() {
    lexer_token_test(
        "trait trait_impl traitimpl impl_trait",
        vec![
            Token::Trait,
            Token::Identifier,
            Token::Identifier,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_super_in_contexts() {
    lexer_token_test(
        "super super_call supercall call_super",
        vec![
            Token::Super,
            Token::Identifier,
            Token::Identifier,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_simple_class_declaration() {
    lexer_token_test("class Graph", vec![Token::Class, Token::Identifier]);
}

#[test]
fn test_class_with_extends() {
    lexer_token_test(
        "class Graph extends BaseGraph",
        vec![
            Token::Class,
            Token::Identifier,
            Token::Extends,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_class_with_implements() {
    lexer_token_test(
        "class Graph implements Traversable",
        vec![
            Token::Class,
            Token::Identifier,
            Token::Implements,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_class_with_extends_and_implements() {
    lexer_token_test(
        "class Graph extends Base implements Trait1, Trait2",
        vec![
            Token::Class,
            Token::Identifier,
            Token::Extends,
            Token::Identifier,
            Token::Implements,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_class_with_generics() {
    lexer_token_test(
        "class Graph<T>",
        vec![
            Token::Class,
            Token::Identifier,
            Token::LessThan,
            Token::Identifier,
            Token::GreaterThan,
        ],
    );
}

#[test]
fn test_class_with_generic_constraint() {
    lexer_token_test(
        "class Graph<T extends Comparable>",
        vec![
            Token::Class,
            Token::Identifier,
            Token::LessThan,
            Token::Identifier,
            Token::Extends,
            Token::Identifier,
            Token::GreaterThan,
        ],
    );
}

#[test]
fn test_simple_trait_declaration() {
    lexer_token_test("trait Drawable", vec![Token::Trait, Token::Identifier]);
}

#[test]
fn test_trait_with_multiple_extends() {
    lexer_token_test(
        "trait Drawable extends Renderable, Colorable",
        vec![
            Token::Trait,
            Token::Identifier,
            Token::Extends,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_super_dot_init() {
    lexer_token_test(
        "super.init()",
        vec![
            Token::Super,
            Token::Dot,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );
}

#[test]
fn test_super_method_call() {
    lexer_token_test(
        "super.method(arg)",
        vec![
            Token::Super,
            Token::Dot,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
        ],
    );
}

#[test]
fn test_class_field_declaration() {
    lexer_token_test(
        "let _graph Map<String, List<String>>",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Identifier,
            Token::LessThan,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::LessThan,
            Token::Identifier,
            Token::GreaterThan,
            Token::GreaterThan,
        ],
    );
}

#[test]
fn test_visibility_modifiers_in_class() {
    lexer_token_test(
        "public fn traverse()",
        vec![
            Token::Public,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );

    lexer_token_test(
        "private let _data int",
        vec![
            Token::Private,
            Token::Let,
            Token::Identifier,
            Token::Identifier,
        ],
    );

    lexer_token_test(
        "protected fn helper()",
        vec![
            Token::Protected,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );
}

#[test]
fn test_class_body_indentation() {
    lexer_token_test(
        "class Graph\n    let x int",
        vec![
            Token::Class,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let,
            Token::Identifier,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_class_multiple_members_indentation() {
    lexer_token_test(
        "class Graph\n    let x int\n    fn init()",
        vec![
            Token::Class,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let,
            Token::Identifier,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_nested_indentation_in_class() {
    lexer_token_test(
        "class Graph\n    fn init()\n        let x = 1",
        vec![
            Token::Class,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_class_case_sensitivity() {
    // Keywords are lowercase only
    lexer_token_test("Class CLASS", vec![Token::Identifier, Token::Identifier]);
}

#[test]
fn test_trait_case_sensitivity() {
    lexer_token_test("Trait TRAIT", vec![Token::Identifier, Token::Identifier]);
}

#[test]
fn test_super_case_sensitivity() {
    lexer_token_test("Super SUPER", vec![Token::Identifier, Token::Identifier]);
}

#[test]
fn test_keyword_operator_boundary() {
    lexer_token_test(
        "class(x)",
        vec![
            Token::Class,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
        ],
    );
    lexer_token_test("super.x", vec![Token::Super, Token::Dot, Token::Identifier]);
}
