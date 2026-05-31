// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::factory::make_type;
use miri::ast::types::Type;
use miri::ast::types::{TypeDeclarationKind, TypeKind};
use miri::ast::{Statement, StatementKind};
use miri::error::compiler::CompilerError;
use miri::lexer::Lexer;
use miri::parser::Parser;
use miri::pipeline::Pipeline;
use miri::type_checker::context::{
    ClassDefinition, FieldInfo, MethodInfo, StructDefinition, TraitDefinition, TypeDefinition,
};
use miri::type_checker::utils::{is_residency_gated_buffer, is_resource};
use miri::type_checker::TypeChecker;
use std::collections::{BTreeMap, HashMap};

pub fn type_checker_test(source: &str) {
    let pipeline = Pipeline::new();
    if let Err(e) = pipeline.frontend(source) {
        panic!("Expected success, but got error: {:?}", e);
    }
}

pub fn type_checker_error_test(source: &str, expected_error: &str) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Expected error '{}', but got success", expected_error),
        Err(CompilerError::TypeErrors { errors, .. }) => {
            let found = errors
                .iter()
                .any(|e| e.to_string().contains(expected_error));
            if !found {
                panic!("Expected error '{}', but got: {:?}", expected_error, errors);
            }
        }
        Err(e) => panic!("Expected TypeErrors, but got: {:?}", e),
    }
}

pub fn type_checker_error_with_help_test(source: &str, expected_error: &str, expected_help: &str) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Expected error '{}', but got success", expected_error),
        Err(CompilerError::TypeErrors { errors, .. }) => {
            let found = errors.iter().any(|e| {
                e.to_string().contains(expected_error) && format!("{:?}", e).contains(expected_help)
            });
            if !found {
                panic!(
                    "Expected error '{}' with help '{}', but got: {:?}",
                    expected_error, expected_help, errors
                );
            }
        }
        Err(e) => panic!("Expected TypeErrors, but got: {:?}", e),
    }
}

pub fn type_checker_errors_test(source: &str, expected_errors: Vec<&str>) {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(_) => panic!("Expected errors, but got success"),
        Err(CompilerError::TypeErrors { errors, .. }) => {
            let error_messages: Vec<String> =
                errors.iter().map(|e| e.to_string().clone()).collect();
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

pub fn type_checker_multi_module_test(modules: Vec<(&str, &str)>) {
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

pub fn type_checker_multi_module_error_test(modules: Vec<(&str, &str)>, expected_error: &str) {
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
            let found = errors
                .iter()
                .any(|e| e.to_string().contains(expected_error));
            if !found {
                panic!("Expected error '{}', but got: {:?}", expected_error, errors);
            }
        }
    }
}

pub fn type_checker_expr_type_test(source: &str, expected_type: Type) {
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

pub fn type_checker_exprs_type_test(cases: Vec<(&str, Type)>) {
    for (source, expected_type) in cases {
        type_checker_expr_type_test(source, expected_type);
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
        StatementKind::FunctionDeclaration(func) => func
            .body
            .as_ref()
            .and_then(|b| find_variable_type_in_statement(b, var_name, type_checker)),
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

pub fn type_checker_vars_type_test(source: &str, expected_types: Vec<(&str, Type)>) {
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
pub fn type_checker_const_type_test(source: &str, expected_types: Vec<(&str, Type)>) {
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
                "Type mismatch for constant '{}'",
                var_name
            );

            // Also verify it's a constant
            assert!(
                result.type_checker.is_constant(var_name),
                "Variable '{}' should be a constant",
                var_name
            );
        } else {
            panic!("Constant '{}' not found or has no initializer", var_name);
        }
    }
}

pub fn type_checker_warning_test(source: &str, expected_warning: &str) {
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

pub fn count_warnings_with_code(source: &str, code: &str) -> usize {
    let pipeline = Pipeline::new();
    let result = match pipeline.frontend(source) {
        Ok(res) => res,
        Err(e) => panic!("Type check failed unexpectedly: {}", e),
    };

    result
        .type_checker
        .warnings
        .iter()
        .filter(|w| w.code == Some(code))
        .count()
}

fn struct_def(has_drop: bool) -> TypeDefinition {
    TypeDefinition::Struct(StructDefinition {
        fields: vec![],
        generics: None,
        module: "test".to_string(),
        has_drop,
    })
}

fn class_def(has_drop: bool) -> TypeDefinition {
    TypeDefinition::Class(ClassDefinition {
        name: "C".to_string(),
        generics: None,
        base_class: None,
        base_class_args: None,
        traits: vec![],
        fields: vec![] as Vec<(String, FieldInfo)>,
        methods: BTreeMap::<String, MethodInfo>::new(),
        module: "test".to_string(),
        is_abstract: false,
        has_drop,
    })
}

fn trait_def() -> TypeDefinition {
    TypeDefinition::Trait(TraitDefinition {
        name: "T".to_string(),
        generics: None,
        parent_traits: vec![],
        parent_trait_args: BTreeMap::new(),
        methods: BTreeMap::<String, MethodInfo>::new(),
        module: "test".to_string(),
    })
}

// `gpu for` residency-gated buffer classification (must track the MIR
// `is_gpu_buffer_capture` predicate: fixed-size `Array` only).

#[test]
fn residency_gated_buffer_accepts_array() {
    assert!(is_residency_gated_buffer(&TypeKind::Custom(
        "Array".to_string(),
        None
    )));
}

#[test]
fn residency_gated_buffer_rejects_list_and_scalar() {
    assert!(!is_residency_gated_buffer(&TypeKind::Custom(
        "List".to_string(),
        None
    )));
    assert!(!is_residency_gated_buffer(&TypeKind::Int));
}

// Generic-parameter classification by constraint.

#[test]
fn unbounded_generic_is_not_resource() {
    let defs: HashMap<String, TypeDefinition> = HashMap::new();
    let g = TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None);
    assert!(!is_resource(&g, &defs));
}

#[test]
fn generic_bounded_by_managed_class_is_not_resource() {
    let mut defs = HashMap::new();
    defs.insert("Greeter".to_string(), class_def(false));
    let bound = make_type(TypeKind::Custom("Greeter".to_string(), None));
    let g = TypeKind::Generic(
        "T".to_string(),
        Some(Box::new(bound)),
        TypeDeclarationKind::Extends,
    );
    assert!(!is_resource(&g, &defs));
}

#[test]
fn generic_bounded_by_resource_class_is_resource() {
    let mut defs = HashMap::new();
    defs.insert("Conn".to_string(), class_def(true));
    let bound = make_type(TypeKind::Custom("Conn".to_string(), None));
    let g = TypeKind::Generic(
        "T".to_string(),
        Some(Box::new(bound)),
        TypeDeclarationKind::Extends,
    );
    assert!(is_resource(&g, &defs));
}

#[test]
fn generic_bounded_by_resource_struct_is_resource() {
    let mut defs = HashMap::new();
    defs.insert("Handle".to_string(), struct_def(true));
    let bound = make_type(TypeKind::Custom("Handle".to_string(), None));
    let g = TypeKind::Generic(
        "T".to_string(),
        Some(Box::new(bound)),
        TypeDeclarationKind::Extends,
    );
    assert!(is_resource(&g, &defs));
}

#[test]
fn generic_bounded_by_trait_is_not_resource() {
    // Traits have no `has_drop` axis today, so a trait-bounded generic is
    // managed-typed.  If a future feature attaches resource
    // semantics to a trait, this test will fail and the classification
    // strategy must be revisited.
    let mut defs = HashMap::new();
    defs.insert("Drawable".to_string(), trait_def());
    let bound = make_type(TypeKind::Custom("Drawable".to_string(), None));
    let g = TypeKind::Generic(
        "T".to_string(),
        Some(Box::new(bound)),
        TypeDeclarationKind::Implements,
    );
    assert!(!is_resource(&g, &defs));
}
