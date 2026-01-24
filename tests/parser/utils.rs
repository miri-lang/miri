// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::vec;

use miri::ast::factory::*;
use miri::ast::types::{Type, TypeKind};
use miri::ast::*;
use miri::error::syntax::{SyntaxError, SyntaxErrorKind};
use miri::lexer::Lexer;
use miri::parser::Parser;

fn parse(input: &str) -> Result<Program, SyntaxError> {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input);

    parser.parse()
}

pub fn parse_program<'src>(input: &'src str) -> Program {
    parse(input).unwrap()
}

pub fn parser_test<'src>(input: &'src str, _expected_body: Vec<Statement>) {
    let program = parse_program(input);
    assert_eq!(
        program,
        Program {
            body: _expected_body
        },
        "Parsing failed for input: {}",
        input
    );
}

pub fn parser_error_test(input: &str, _expected_error: &SyntaxErrorKind) {
    let parse_result = parse(input);
    assert!(parse_result.is_err());
    assert_eq!(parse_result.unwrap_err().kind, *_expected_error);
}

pub fn variable_declaration_test(
    input: &str,
    expected: Vec<VariableDeclaration>,
    visibility: MemberVisibility,
) {
    parser_test(input, vec![variable_statement(expected, visibility)]);
}

pub fn literal_test(input: &str, expected: Literal) {
    parser_test(input, vec![expression_statement(literal(expected))]);
}

pub fn run_literal_tests(inputs: Vec<(&str, Literal)>) {
    for (input, expected) in inputs {
        literal_test(input, expected);
    }
}

pub fn integer_test(input: &str, expected: IntegerLiteral) {
    literal_test(input, Literal::Integer(expected));
}

pub fn run_int_tests(inputs: Vec<(&str, IntegerLiteral)>) {
    for (input, expected) in inputs {
        integer_test(input, expected);
    }
}

pub fn float_test(input: &str, expected: FloatLiteral) {
    literal_test(input, Literal::Float(expected));
}

pub fn run_float_tests(inputs: Vec<(&str, FloatLiteral)>) {
    for (input, expected) in inputs {
        float_test(input, expected);
    }
}

pub fn binary_expression_test(input: &str, left: Expression, op: BinaryOp, right: Expression) {
    parser_test(input, vec![expression_statement(binary(left, op, right))]);
}

pub fn assignment_expression_test(
    input: &str,
    left: LeftHandSideExpression,
    op: AssignmentOp,
    right: Expression,
) {
    parser_test(input, vec![expression_statement(assign(left, op, right))]);
}

pub fn if_statement_test(
    input: &str,
    condition: Expression,
    then_block: Statement,
    else_block: Option<Statement>,
    if_statement_type: IfStatementType,
) {
    parser_test(
        input,
        vec![stmt(StatementKind::If(
            Box::new(condition),
            Box::new(then_block),
            else_block.map(Box::new),
            if_statement_type,
        ))],
    );
}

pub fn combined_if_unless_test(
    input: &str,
    condition: Expression,
    then_block: Statement,
    else_block: Option<Statement>,
) {
    if_statement_test(
        input,
        condition.clone(),
        then_block.clone(),
        else_block.clone(),
        IfStatementType::If,
    );
    if_statement_test(
        input.replace("if", "unless").as_str(),
        condition,
        then_block,
        else_block,
        IfStatementType::Unless,
    );
}

pub fn unary_expression_test(input: &str, op: UnaryOp, right: Expression) {
    parser_test(input, vec![expression_statement(unary(op, right))]);
}

pub fn while_expression_test(
    input: &str,
    condition: Expression,
    then_block: Statement,
    while_statement_type: WhileStatementType,
) {
    parser_test(
        input,
        vec![while_statement_with_type(
            condition,
            then_block,
            while_statement_type,
        )],
    );
}

pub fn combined_while_until_test(input: &str, condition: Expression, then_block: Statement) {
    while_expression_test(
        input,
        condition.clone(),
        then_block.clone(),
        WhileStatementType::While,
    );
    while_expression_test(
        input.replace("while", "until").as_str(),
        condition,
        then_block,
        WhileStatementType::Until,
    );
}

pub fn combined_do_while_until_test(input: &str, condition: Expression, then_block: Statement) {
    while_expression_test(
        input,
        condition.clone(),
        then_block.clone(),
        WhileStatementType::DoWhile,
    );
    while_expression_test(
        input.replace("while", "until").as_str(),
        condition,
        then_block,
        WhileStatementType::DoUntil,
    );
}

pub fn for_statement_test(
    input: &str,
    variable_declarations: Vec<VariableDeclaration>,
    iterable: Expression,
    body: Statement,
) {
    parser_test(
        input,
        vec![for_statement(variable_declarations, iterable, body)],
    );
}

pub fn type_statement_test(type_str: &str, expected: Expression) {
    let input = format!("let x {}", type_str);
    variable_declaration_test(
        &input,
        vec![let_variable("x", opt_expr(expected), None)],
        MemberVisibility::Public,
    );
}

pub fn run_parser_error_tests(inputs: Vec<&str>, expected_kind: &SyntaxErrorKind) {
    for input in inputs {
        parser_error_test(input, expected_kind);
    }
}

pub fn type_list_expr(inner: Expression) -> Type {
    make_type(TypeKind::List(Box::new(inner)))
}

pub fn type_map_expr(key: Expression, value: Expression) -> Type {
    make_type(TypeKind::Map(Box::new(key), Box::new(value)))
}

pub fn type_tuple_expr(elements: Vec<Expression>) -> Type {
    make_type(TypeKind::Tuple(elements))
}

pub fn class_statement_test(
    input: &str,
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) {
    parser_test(
        input,
        vec![class_statement(
            name,
            generic_types,
            base_class,
            traits,
            body,
            visibility,
        )],
    );
}

pub fn trait_statement_test(
    input: &str,
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    parent_traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) {
    parser_test(
        input,
        vec![trait_statement(
            name,
            generic_types,
            parent_traits,
            body,
            visibility,
        )],
    );
}

pub fn abstract_class_statement_test(
    input: &str,
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) {
    parser_test(
        input,
        vec![abstract_class_statement(
            name,
            generic_types,
            base_class,
            traits,
            body,
            visibility,
        )],
    );
}

// ============= Edge Case Utilities =============

/// Assert that a program parses successfully and has the expected number of statements
pub fn assert_statement_count(input: &str, expected_count: usize) {
    let program = parse_program(input);
    assert_eq!(
        program.body.len(),
        expected_count,
        "Expected {} statements, got {} for input:\n{}",
        expected_count,
        program.body.len(),
        input
    );
}

/// Assert that deeply nested while loops parse correctly and have the expected structure.
/// Traverses the AST to verify the nesting depth and that the innermost statement is a break.
pub fn assert_nested_while_structure(input: &str, expected_depth: usize) {
    let program = parse_program(input);
    assert_eq!(
        program.body.len(),
        1,
        "Expected exactly 1 top-level statement"
    );

    let mut current_stmt = &program.body[0];
    for depth in 0..expected_depth {
        match &current_stmt.node {
            StatementKind::While(_, body, ..) => {
                // For inline syntax, body is directly the inner statement
                // For block syntax, body is a Block containing statements
                match &body.node {
                    StatementKind::Block(stmts) => {
                        assert_eq!(
                            stmts.len(),
                            1,
                            "Expected block with 1 statement at depth {}",
                            depth
                        );
                        current_stmt = &stmts[0];
                    }
                    StatementKind::While(..) => {
                        // Inline syntax - body is directly the next while
                        current_stmt = body;
                    }
                    StatementKind::Break => {
                        assert_eq!(
                            depth,
                            expected_depth - 1,
                            "Found break at depth {}, expected at depth {}",
                            depth,
                            expected_depth - 1
                        );
                        return;
                    }
                    other => {
                        panic!(
                            "Expected While, Block, or Break at depth {}, found {:?}",
                            depth, other
                        );
                    }
                }
            }
            StatementKind::Break => {
                assert_eq!(
                    depth, expected_depth,
                    "Found break at depth {}, expected at depth {}",
                    depth, expected_depth
                );
                return;
            }
            other => {
                panic!(
                    "Expected While statement at depth {}, found {:?}",
                    depth, other
                );
            }
        }
    }
}

/// Assert that deeply nested if statements parse correctly.
/// Traverses the AST to verify the nesting depth.
pub fn assert_nested_if_structure(input: &str, expected_depth: usize) {
    let program = parse_program(input);
    assert_eq!(
        program.body.len(),
        1,
        "Expected exactly 1 top-level statement"
    );

    let mut current_stmt = &program.body[0];
    for depth in 0..expected_depth {
        match &current_stmt.node {
            StatementKind::If(_, then_block, ..) => match &then_block.node {
                StatementKind::Block(stmts) => {
                    assert_eq!(
                        stmts.len(),
                        1,
                        "Expected block with 1 statement at depth {}",
                        depth
                    );
                    current_stmt = &stmts[0];
                }
                StatementKind::If(..) => {
                    current_stmt = then_block;
                }
                StatementKind::Expression(_) => {
                    assert_eq!(
                        depth,
                        expected_depth - 1,
                        "Found expression at depth {}, expected at depth {}",
                        depth,
                        expected_depth - 1
                    );
                    return;
                }
                other => {
                    panic!(
                        "Expected If, Block, or Expression at depth {}, found {:?}",
                        depth, other
                    );
                }
            },
            StatementKind::Expression(_) => {
                assert_eq!(
                    depth, expected_depth,
                    "Found expression at depth {}, expected at depth {}",
                    depth, expected_depth
                );
                return;
            }
            other => {
                panic!(
                    "Expected If statement at depth {}, found {:?}",
                    depth, other
                );
            }
        }
    }
}

/// Assert that deeply nested for loops parse correctly.
pub fn assert_nested_for_structure(input: &str, expected_depth: usize) {
    let program = parse_program(input);
    assert_eq!(
        program.body.len(),
        1,
        "Expected exactly 1 top-level statement"
    );

    let mut current_stmt = &program.body[0];
    for depth in 0..expected_depth {
        match &current_stmt.node {
            StatementKind::For(_, _, body) => match &body.node {
                StatementKind::Block(stmts) => {
                    assert_eq!(
                        stmts.len(),
                        1,
                        "Expected block with 1 statement at depth {}",
                        depth
                    );
                    current_stmt = &stmts[0];
                }
                StatementKind::For(..) => {
                    current_stmt = body;
                }
                StatementKind::Expression(_) | StatementKind::Empty => {
                    assert_eq!(
                        depth,
                        expected_depth - 1,
                        "Found terminal statement at depth {}, expected at depth {}",
                        depth,
                        expected_depth - 1
                    );
                    return;
                }
                other => {
                    panic!(
                        "Expected For, Block, or terminal at depth {}, found {:?}",
                        depth, other
                    );
                }
            },
            StatementKind::Expression(_) | StatementKind::Empty => {
                assert_eq!(
                    depth, expected_depth,
                    "Found terminal at depth {}, expected at depth {}",
                    depth, expected_depth
                );
                return;
            }
            other => {
                panic!(
                    "Expected For statement at depth {}, found {:?}",
                    depth, other
                );
            }
        }
    }
}

/// Assert that a function declaration parses with the expected number of parameters
pub fn assert_function_parameter_count(input: &str, expected_count: usize) {
    let program = parse_program(input);
    assert_eq!(
        program.body.len(),
        1,
        "Expected exactly 1 top-level statement"
    );

    match &program.body[0].node {
        StatementKind::FunctionDeclaration(_, _, params, ..) => {
            assert_eq!(
                params.len(),
                expected_count,
                "Expected {} parameters, got {}",
                expected_count,
                params.len()
            );
        }
        other => {
            panic!("Expected function declaration, found {:?}", other);
        }
    }
}
