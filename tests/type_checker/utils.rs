// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::compiler_error::CompilerError;
use miri::pipeline::Pipeline;
use miri::ast::{Statement, Type};

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
                     panic!("Expected error '{}' not found. Found: {:?}", expected, error_messages);
                }
            }
        }
        Err(e) => panic!("Expected TypeErrors, but got: {:?}", e),
    }
}

pub fn check_expr_type(source: &str, expected_type: Type) {
    let pipeline = Pipeline::new();
    let result = match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    };

    let last_stmt = result.ast.body.iter().rev().find(|s| match s {
        Statement::Empty => false,
        Statement::Block(stmts) if stmts.is_empty() => false,
        _ => true,
    }).expect("Program is empty or only contains empty statements");

    if let Statement::Expression(expr) = last_stmt {
            let actual_type = result.type_checker.get_type(expr.id).expect("Type not found for expression");
            assert_eq!(actual_type, &expected_type, "Type mismatch for expression '{}'", source);
    } else {
        panic!("Last statement is not an expression in '{}'. Found: {:?}", source, last_stmt);
    }
}

pub fn check_exprs_type(cases: Vec<(&str, Type)>) {
    for (source, expected_type) in cases {
        check_expr_type(source, expected_type);
    }
}

pub fn check_vars_type(source: &str, expected_types: Vec<(&str, Type)>) {
    let pipeline = Pipeline::new();
    let result = match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    };
    
    for (var_name, expected_type) in expected_types {
        let mut found = false;
        for statement in &result.ast.body {
            if let Statement::Variable(decls, _) = statement {
                for decl in decls {
                    if decl.name == var_name {
                        if let Some(init) = &decl.initializer {
                            if let Some(actual_type) = result.type_checker.get_type(init.id) {
                                assert_eq!(actual_type, &expected_type, "Type mismatch for variable '{}'", var_name);
                                found = true;
                            }
                        }
                    }
                }
            }
        }
        assert!(found, "Variable '{}' not found or has no initializer", var_name);
    }
}
