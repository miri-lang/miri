// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::ast::*;
use miri::lexer::Lexer;
use miri::parser::Parser;
use miri::syntax_error::{SyntaxError, SyntaxErrorKind};

use super::ast_builder::*;


pub fn parse(input: &str) -> Result<Program, SyntaxError> {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input, AstFactory::new());

    parser.parse()
}

pub fn parse_program<'src>(input: &'src str) -> Program {
    parse(input).unwrap()
}

pub fn parse_test<'src>(input: &'src str, _expected_body: Vec<Statement>) {
    let program = parse_program(input);
    assert_eq!(program, Program {
        body: _expected_body
    }, "Parsing failed for input: {}", input);
}

pub fn parse_error_test<'src>(input: &'src str, _expected_error: SyntaxErrorKind) {
    let parse_result = parse(input);
    assert!(parse_result.is_err());
    assert_eq!(parse_result.unwrap_err().kind, _expected_error);
}

pub fn parse_variable_declaration_test(input: &str, expected: Vec<VariableDeclaration>, visibility: MemberVisibility) {
    parse_test(input, vec![
        Statement::Variable(expected, visibility)
    ]);
}

pub fn parse_literal_test(input: &str, expected: Literal) {
    parse_test(input, vec![
        Statement::Expression(Expression::Literal(expected))
    ]);
}

pub fn parse_integer_test(input: &str, expected: IntegerLiteral) {
    parse_literal_test(input, Literal::Integer(expected));
}

pub fn parse_float_test(input: &str, expected: FloatLiteral) {
    parse_literal_test(input, Literal::Float(expected));
}

pub fn parse_binary_expression_test(input: &str, left: Expression, op: BinaryOp, right: Expression) {
    parse_test(input, vec![
        Statement::Expression(Expression::Binary(Box::new(left), op, Box::new(right)))
    ]);
}

pub fn parse_assignment_expression_test(input: &str, left: LeftHandSideExpression, op: AssignmentOp, right: Expression) {
    parse_test(input, vec![
        Statement::Expression(Expression::Assignment(Box::new(left), op, Box::new(right)))
    ]);
}

pub fn parse_if_statement_test(input: &str, condition: Expression, then_block: Statement, else_block: Option<Statement>, if_statement_type: IfStatementType) {
    parse_test(input, vec![
        Statement::If(Box::new(condition), Box::new(then_block), else_block.map(Box::new), if_statement_type)
    ]);
}

pub fn parse_if_test(input: &str, condition: Expression, then_block: Statement, else_block: Option<Statement>) {
    parse_if_statement_test(input, condition.clone(), then_block.clone(), else_block.clone(), IfStatementType::If);
    parse_if_statement_test(input.replace("if", "unless").as_str(), condition, then_block, else_block, IfStatementType::Unless);
}

pub fn parse_unary_expression_test(input: &str, op: UnaryOp, right: Expression) {
    parse_test(input, vec![
        Statement::Expression(Expression::Unary(op, Box::new(right)))
    ]);
}

pub fn parse_while_expression_test(input: &str, condition: Expression, then_block: Statement, while_statement_type: WhileStatementType) {
    parse_test(input, vec![
        Statement::While(Box::new(condition), Box::new(then_block), while_statement_type)
    ]);
}

pub fn parse_while_test(input: &str, condition: Expression, then_block: Statement) {
    parse_while_expression_test(input, condition.clone(), then_block.clone(), WhileStatementType::While);
    parse_while_expression_test(input.replace("while", "until").as_str(), condition, then_block, WhileStatementType::Until);
}

pub fn parse_for_test(input: &str, variable_declarations: Vec<VariableDeclaration>, iterable: Expression, body: Statement) {
    parse_test(input, vec![
        Statement::For(variable_declarations, Box::new(iterable), Box::new(body))
    ]);
}

pub fn parse_type_test(type_str: &str, expected: Expression) {
    let input = format!("let x {}", type_str);
    parse_variable_declaration_test(&input, vec![
        let_variable(
            "x",
            opt_expr(expected),
            None
        )
    ], MemberVisibility::Public);
}
