// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::types::Type;
use miri::ast::{Statement, StatementKind};
use miri::error::compiler::CompilerError;
use miri::lexer::Lexer;
use miri::parser::Parser;
use miri::pipeline::Pipeline;
use miri::type_checker::TypeChecker;

pub fn check_success(source: &str) {
    let pipeline = Pipeline::new();
    if let Err(e) = pipeline.frontend(source) {
        panic!("Expected success, but got error: {:?}", e);
    }
}

pub fn check_error(source: &str, expected_error: &str) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Expected error '{}', but got success", expected_error),
        Err(CompilerError::TypeErrors(errors)) => {
            let found = errors.iter().any(|e| e.message.contains(expected_error));
            if !found {
                panic!("Expected error '{}', but got: {:?}", expected_error, errors);
            }
        }
        Err(e) => panic!("Expected TypeErrors, but got: {:?}", e),
    }
}

pub fn check_errors(source: &str, expected_errors: Vec<&str>) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Expected errors, but got success"),
        Err(CompilerError::TypeErrors(errors)) => {
            let error_messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
            for expected in expected_errors {
                if !error_messages.iter().any(|msg| msg.contains(expected)) {
                    panic!(
                        "Expected error '{}' not found. Found: {:?}",
                        expected, error_messages
                    );
                }
            }
        }
        Err(e) => panic!("Expected TypeErrors, but got: {:?}", e),
    }
}

pub fn check_multi_module_success(modules: Vec<(&str, &str)>) {
    let mut type_checker = TypeChecker::new();

    for (module_name, source) in modules {
        type_checker.set_current_module(module_name.to_string());

        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let program = parser.parse().expect("Failed to parse module");

        if let Err(errors) = type_checker.check(&program) {
            panic!("Type check failed for module {}: {:?}", module_name, errors);
        }
    }
}

pub fn check_multi_module_error(modules: Vec<(&str, &str)>, expected_error: &str) {
    let mut type_checker = TypeChecker::new();
    let mut last_result = Ok(());

    for (module_name, source) in modules {
        type_checker.set_current_module(module_name.to_string());

        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let program = parser.parse().expect("Failed to parse module");

        last_result = type_checker.check(&program);
    }

    match last_result {
        Ok(_) => panic!("Expected error '{}', but got success", expected_error),
        Err(errors) => {
            let found = errors.iter().any(|e| e.message.contains(expected_error));
            if !found {
                panic!("Expected error '{}', but got: {:?}", expected_error, errors);
            }
        }
    }
}

pub fn check_expr_type(source: &str, expected_type: Type) {
    let pipeline = Pipeline::new();
    let result = match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    };

    let last_stmt = result
        .ast
        .body
        .iter()
        .rev()
        .find(|s| match &s.node {
            StatementKind::Empty => false,
            StatementKind::Block(stmts) if stmts.is_empty() => false,
            _ => true,
        })
        .expect("Program is empty or only contains empty statements");

    if let StatementKind::Expression(expr) = &last_stmt.node {
        let actual_type = result
            .type_checker
            .get_type(expr.id)
            .expect("Type not found for expression");
        assert_eq!(
            actual_type, &expected_type,
            "Type mismatch for expression '{}'",
            source
        );
    } else {
        panic!(
            "Last statement is not an expression in '{}'. Found: {:?}",
            source, last_stmt
        );
    }
}

pub fn check_exprs_type(cases: Vec<(&str, Type)>) {
    for (source, expected_type) in cases {
        check_expr_type(source, expected_type);
    }
}

fn find_variable_type_in_statement(
    stmt: &Statement,
    var_name: &str,
    type_checker: &TypeChecker,
) -> Option<Type> {
    match &stmt.node {
        StatementKind::Variable(decls, _) => {
            for decl in decls {
                if decl.name == var_name {
                    if let Some(init) = &decl.initializer {
                        return type_checker.get_type(init.id).cloned();
                    }
                }
            }
            None
        }
        StatementKind::Block(stmts) => {
            find_variable_type_in_statements(stmts, var_name, type_checker)
        }
        StatementKind::If(_, then_block, else_block, _) => {
            find_variable_type_in_statement(then_block, var_name, type_checker).or_else(|| {
                else_block
                    .as_ref()
                    .and_then(|s| find_variable_type_in_statement(s, var_name, type_checker))
            })
        }
        StatementKind::While(_, body, _) => {
            find_variable_type_in_statement(body, var_name, type_checker)
        }
        StatementKind::For(_, _, body) => {
            find_variable_type_in_statement(body, var_name, type_checker)
        }
        StatementKind::FunctionDeclaration(_, _, _, _, body, _) => {
            find_variable_type_in_statement(body, var_name, type_checker)
        }
        _ => None,
    }
}

fn find_variable_type_in_statements(
    stmts: &[Statement],
    var_name: &str,
    type_checker: &TypeChecker,
) -> Option<Type> {
    for stmt in stmts {
        if let Some(ty) = find_variable_type_in_statement(stmt, var_name, type_checker) {
            return Some(ty);
        }
    }
    None
}

pub fn check_vars_type(source: &str, expected_types: Vec<(&str, Type)>) {
    let pipeline = Pipeline::new();
    let result = match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    };

    for (var_name, expected_type) in expected_types {
        let actual_type = if let Some(ty) = result.type_checker.get_variable_type(var_name) {
            Some(ty.clone())
        } else {
            find_variable_type_in_statements(&result.ast.body, var_name, &result.type_checker)
        };

        if let Some(ty) = actual_type {
            assert_eq!(
                &ty, &expected_type,
                "Type mismatch for variable '{}'",
                var_name
            );
        } else {
            panic!("Variable '{}' not found or has no initializer", var_name);
        }
    }
}

pub fn check_warning(source: &str, expected_warning: &str) {
    let pipeline = Pipeline::new();
    let result = match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    };

    let found = result
        .type_checker
        .warnings
        .iter()
        .any(|w| w.message.contains(expected_warning));

    if !found {
        let warning_messages: Vec<String> = result
            .type_checker
            .warnings
            .iter()
            .map(|w| w.message.clone())
            .collect();
        panic!(
            "Expected warning '{}' not found. Found: {:?}",
            expected_warning, warning_messages
        );
    }
}
