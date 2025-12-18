// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::ast::*;
use miri::ast_factory::*;
use miri::lexer::Lexer;
use miri::parser::Parser;
use miri::syntax_error::{SyntaxError, SyntaxErrorKind};

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

pub fn parser_error_test<'src>(input: &'src str, _expected_error: &SyntaxErrorKind) {
    let parse_result = parse(input);
    assert!(parse_result.is_err());
    assert_eq!(parse_result.unwrap_err().kind, *_expected_error);
}

pub fn variable_declaration_test(
    input: &str,
    expected: Vec<VariableDeclaration>,
    visibility: MemberVisibility,
) {
    parser_test(input, vec![Statement::Variable(expected, visibility)]);
}

pub fn literal_test(input: &str, expected: Literal) {
    parser_test(input, vec![Statement::Expression(literal(expected))]);
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
    parser_test(input, vec![Statement::Expression(binary(left, op, right))]);
}

pub fn assignment_expression_test(
    input: &str,
    left: LeftHandSideExpression,
    op: AssignmentOp,
    right: Expression,
) {
    parser_test(input, vec![Statement::Expression(assign(left, op, right))]);
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
        vec![Statement::If(
            Box::new(condition),
            Box::new(then_block),
            else_block.map(Box::new),
            if_statement_type,
        )],
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
    parser_test(input, vec![Statement::Expression(unary(op, right))]);
}

pub fn while_expression_test(
    input: &str,
    condition: Expression,
    then_block: Statement,
    while_statement_type: WhileStatementType,
) {
    parser_test(
        input,
        vec![Statement::While(
            Box::new(condition),
            Box::new(then_block),
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
        vec![Statement::For(
            variable_declarations,
            Box::new(iterable),
            Box::new(body),
        )],
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
        parser_error_test(input, &expected_kind);
    }
}
