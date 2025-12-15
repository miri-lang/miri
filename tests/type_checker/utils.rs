// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::pipeline::{Pipeline, PipelineResult};
use miri::ast::{Statement, Type};

pub fn type_check_test(source: &str) -> PipelineResult {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    }
}

pub fn assert_expression_type(source: &str, expected_type: Type) {
    let result = type_check_test(source);
    let last_stmt = result.ast.body.last().expect("Program is empty");
    if let Statement::Expression(expr) = last_stmt {
            let actual_type = result.type_checker.get_type(expr.id).expect("Type not found for expression");
            assert_eq!(actual_type, &expected_type, "Type mismatch for expression '{}'", source);
    } else {
        panic!("Last statement is not an expression in '{}'", source);
    }
}

pub fn assert_expressions_type(cases: Vec<(&str, Type)>) {
    for (source, expected_type) in cases {
        assert_expression_type(source, expected_type);
    }
}

pub fn assert_variable_types(source: &str, expected_types: Vec<(&str, Type)>) {
    let result = type_check_test(source);
    
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

pub fn assert_type_check_error(source: &str, expected_error_part: &str) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Type check should have failed but succeeded"),
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains(expected_error_part), "Error message '{}' did not contain '{}'", msg, expected_error_part);
        }
    }
}