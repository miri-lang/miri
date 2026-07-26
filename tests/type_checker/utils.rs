// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::factory::make_type;
use miri::ast::types::Type;
use miri::ast::types::{TypeDeclarationKind, TypeKind};
use miri::ast::{Statement, StatementKind};
use miri::error::compiler::CompilerError;
use miri::lexer::Lexer;
use miri::parser::Parser;
use miri::pipeline::{Pipeline, PipelineResult};
use miri::type_checker::context::{
    ClassDefinition, FieldInfo, MethodInfo, StructDefinition, TraitDefinition, TypeDefinition,
};
use miri::type_checker::utils::{is_residency_gated_buffer, is_resource};
use miri::type_checker::TypeChecker;
use std::collections::{BTreeMap, HashMap};

/// Runs the frontend over `source` and hands back its result, panicking if the
/// source does not type-check. This is the single entry point for tests that
/// need to inspect what the type checker recorded (inferred types, warnings,
/// generic-class instantiations, GPU buffer initializers); they must not build a
/// `Pipeline` themselves, so the coupling to the pipeline API stays in one file.
pub fn type_checker_result(source: &str) -> PipelineResult {
    let pipeline = Pipeline::new();
    match pipeline.frontend(source) {
        Ok(result) => result,
        Err(e) => panic!("Expected success, but got error: {:?}", e),
    }
}

pub fn type_checker_test(source: &str) {
    type_checker_result(source);
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
    let result = type_checker_result(source);

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
    let result = type_checker_result(source);

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
    let result = type_checker_result(source);

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
    let result = type_checker_result(source);

    let found = result
        .type_checker
        .diagnostics
        .warnings
        .iter()
        .any(|w| w.message.contains(expected_warning));

    if !found {
        let warning_messages: Vec<String> = result
            .type_checker
            .diagnostics
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
    let result = type_checker_result(source);

    result
        .type_checker
        .diagnostics
        .warnings
        .iter()
        .filter(|w| w.code == Some(code))
        .count()
}

fn struct_def(has_drop: bool) -> TypeDefinition {
    TypeDefinition::Struct(StructDefinition {
        fields: vec![],
        generics: None,
        traits: vec![],
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
        trait_args: std::collections::HashMap::new(),
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

// `forall` residency-gated buffer classification (must track the MIR
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

// GPU type-predicate coherence.
//
// `gpu_scalar_class` is the single source of truth for scalar-leaf device
// eligibility; the three GPU type predicates (`is_gpu_compatible`,
// `is_gpu_buffer_element`, and the accelerable element bound) all derive from
// it, so they can never disagree on a scalar. The matrix below is exhaustive
// over every `TypeKind` — the classification match has no wildcard, so a new
// variant cannot compile until it is classified — and asserts the capability
// ladder `Storage ⊂ kernel-usable`.

use miri::ast::factory::type_expr_non_null;
use miri::ast::types::FunctionTypeData;
use miri::type_checker::utils::{
    gpu_scalar_class, is_accelerable, is_gpu_buffer_element, is_gpu_compatible, GpuScalarClass,
};

/// The expected scalar class of every `TypeKind`, restated independently of the
/// production classifier. The match is exhaustive (no `_`): adding a `TypeKind`
/// variant forces a deliberate classification decision here.
fn expected_scalar_class(kind: &TypeKind) -> GpuScalarClass {
    match kind {
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64 => GpuScalarClass::Storage,

        TypeKind::Boolean | TypeKind::Void | TypeKind::Error | TypeKind::I128 | TypeKind::U128 => {
            GpuScalarClass::KernelOnly
        }

        TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _) => GpuScalarClass::Forbidden,
    }
}

/// One representative value of every `TypeKind` variant — the universe the
/// coherence matrix ranges over.
fn one_of_every_type_kind() -> Vec<TypeKind> {
    let elem = || Box::new(type_expr_non_null(make_type(TypeKind::Int)));
    let ty = || Box::new(make_type(TypeKind::Int));
    vec![
        TypeKind::Int,
        TypeKind::I8,
        TypeKind::I16,
        TypeKind::I32,
        TypeKind::I64,
        TypeKind::I128,
        TypeKind::U8,
        TypeKind::U16,
        TypeKind::U32,
        TypeKind::U64,
        TypeKind::U128,
        TypeKind::Float,
        TypeKind::F16,
        TypeKind::F32,
        TypeKind::F64,
        TypeKind::Boolean,
        TypeKind::Void,
        TypeKind::Error,
        TypeKind::String,
        TypeKind::Identifier,
        TypeKind::RawPtr,
        TypeKind::List(elem()),
        TypeKind::Array(elem(), elem()),
        TypeKind::Map(elem(), elem()),
        TypeKind::Set(elem()),
        TypeKind::Tuple(vec![type_expr_non_null(make_type(TypeKind::Int))]),
        TypeKind::Result(elem(), elem()),
        TypeKind::Future(elem()),
        TypeKind::Option(ty()),
        TypeKind::Meta(ty()),
        TypeKind::Linear(ty()),
        TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: vec![],
            return_type: None,
        })),
        TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None),
        TypeKind::Custom("Foo".to_string(), None),
    ]
}

#[test]
fn gpu_scalar_class_agrees_with_independent_oracle() {
    for kind in one_of_every_type_kind() {
        assert_eq!(
            gpu_scalar_class(&kind),
            expected_scalar_class(&kind),
            "gpu_scalar_class disagrees for {:?}",
            kind
        );
    }
}

#[test]
fn storage_class_is_a_subset_of_kernel_compatible() {
    // Every storage-buffer element must also be kernel-body compatible: a value
    // the device can persist must be a value the kernel can name.
    for kind in one_of_every_type_kind() {
        if is_gpu_buffer_element(&kind) {
            assert!(
                is_gpu_compatible(&kind),
                "{:?} is a buffer element but not GPU-compatible",
                kind
            );
        }
    }
}

#[test]
fn scalar_predicates_are_coherent_for_every_scalar_leaf() {
    let defs: HashMap<String, TypeDefinition> = HashMap::new();
    for kind in one_of_every_type_kind() {
        let class = gpu_scalar_class(&kind);
        if class == GpuScalarClass::Forbidden {
            // Containers/context/generics are classified by their owning
            // predicate, not by the scalar rule — skip them here.
            continue;
        }
        let is_storage = class == GpuScalarClass::Storage;
        // A scalar leaf is always kernel-body compatible.
        assert!(
            is_gpu_compatible(&kind),
            "scalar leaf {:?} must be GPU-compatible",
            kind
        );
        // Storage membership is the single gate for buffer-element AND binding
        // eligibility — no scalar is accepted by one and rejected by the other.
        assert_eq!(
            is_gpu_buffer_element(&kind),
            is_storage,
            "buffer-element disagrees with scalar class for {:?}",
            kind
        );
        assert_eq!(
            is_accelerable(&kind, &defs),
            is_storage,
            "accelerable disagrees with scalar class for {:?}",
            kind
        );
    }
}

#[test]
fn boolean_is_kernel_only_not_bindable() {
    // `Boolean` was once accelerable (accepted at the
    // binding) yet not a buffer element (rejected at capture). It must now be
    // kernel-local only — usable inside a kernel, never a device buffer leaf.
    let defs: HashMap<String, TypeDefinition> = HashMap::new();
    assert_eq!(
        gpu_scalar_class(&TypeKind::Boolean),
        GpuScalarClass::KernelOnly
    );
    assert!(is_gpu_compatible(&TypeKind::Boolean));
    assert!(!is_gpu_buffer_element(&TypeKind::Boolean));
    assert!(!is_accelerable(&TypeKind::Boolean, &defs));
}

// Accelerator binding registry: how a host type binds on the device
// (`accelerable_binding_kind`) and how many bytes one element occupies
// (`accelerable_byte_size`).

use miri::ast::common::MemberVisibility;
use miri::ast::types::{
    BuiltinCollectionKind, ACCELERABLE_TRAIT_NAME, VEC3_TYPE_NAME, VEC4_TYPE_NAME,
};
use miri::type_checker::context::AliasDefinition;
use miri::type_checker::utils::{
    accelerable_binding_kind, accelerable_byte_size, AcceleratorBindingKind,
};

fn array_name() -> &'static str {
    BuiltinCollectionKind::Array.name()
}

fn list_name() -> &'static str {
    BuiltinCollectionKind::List.name()
}

/// Builds a non-generic type alias whose underlying template is `template`,
/// e.g. `alias(TypeKind::U8)` models `type Byte is u8`.
fn alias(template: TypeKind) -> TypeDefinition {
    TypeDefinition::Alias(AliasDefinition {
        template: make_type(template),
        generics: None,
    })
}

fn custom(name: &str, args: Vec<TypeKind>) -> TypeKind {
    TypeKind::Custom(
        name.to_string(),
        Some(
            args.into_iter()
                .map(|kind| type_expr_non_null(make_type(kind)))
                .collect(),
        ),
    )
}

fn no_defs() -> HashMap<String, TypeDefinition> {
    HashMap::new()
}

#[test]
fn binding_kind_scalars_are_uniforms() {
    for kind in [
        TypeKind::Int,
        TypeKind::I8,
        TypeKind::I32,
        TypeKind::I64,
        TypeKind::U16,
        TypeKind::Float,
        TypeKind::F32,
        TypeKind::F64,
        // Kernel-only scalars still ride as uniforms even though they are
        // not storage-buffer elements — matches the forall capture split.
        TypeKind::Boolean,
        TypeKind::I128,
    ] {
        assert_eq!(
            accelerable_binding_kind(&kind),
            Some(AcceleratorBindingKind::Uniform),
            "{kind:?} should bind as a uniform"
        );
    }
}

#[test]
fn binding_kind_collections_and_aggregates_are_storage() {
    let cases = [
        custom(array_name(), vec![TypeKind::I32, TypeKind::Int]),
        custom(list_name(), vec![TypeKind::F32]),
        TypeKind::Tuple(vec![
            type_expr_non_null(make_type(TypeKind::I32)),
            type_expr_non_null(make_type(TypeKind::F32)),
        ]),
    ];
    for kind in cases {
        assert_eq!(
            accelerable_binding_kind(&kind),
            Some(AcceleratorBindingKind::Storage),
            "{kind:?} should bind as storage"
        );
    }
}

#[test]
fn binding_kind_vectors_are_uniforms() {
    assert_eq!(
        accelerable_binding_kind(&custom(VEC3_TYPE_NAME, vec![TypeKind::F32])),
        Some(AcceleratorBindingKind::Uniform)
    );
}

#[test]
fn binding_kind_unbindable_types_are_none() {
    assert_eq!(accelerable_binding_kind(&TypeKind::String), None);
    assert_eq!(
        accelerable_binding_kind(&TypeKind::Map(
            Box::new(type_expr_non_null(make_type(TypeKind::Int))),
            Box::new(type_expr_non_null(make_type(TypeKind::Int))),
        )),
        None
    );
}

#[test]
fn byte_size_scalars_use_host_widths() {
    let defs = no_defs();
    assert_eq!(accelerable_byte_size(&TypeKind::Int, &defs), Some(8));
    assert_eq!(accelerable_byte_size(&TypeKind::I8, &defs), Some(1));
    assert_eq!(accelerable_byte_size(&TypeKind::U16, &defs), Some(2));
    assert_eq!(accelerable_byte_size(&TypeKind::I32, &defs), Some(4));
    assert_eq!(accelerable_byte_size(&TypeKind::I64, &defs), Some(8));
    assert_eq!(accelerable_byte_size(&TypeKind::Float, &defs), Some(8));
    assert_eq!(accelerable_byte_size(&TypeKind::F32, &defs), Some(4));
    assert_eq!(accelerable_byte_size(&TypeKind::F64, &defs), Some(8));
}

#[test]
fn byte_size_non_accelerable_scalars_are_none() {
    let defs = no_defs();
    assert_eq!(accelerable_byte_size(&TypeKind::Boolean, &defs), None);
    assert_eq!(accelerable_byte_size(&TypeKind::I128, &defs), None);
}

#[test]
fn byte_size_collection_reports_element_width() {
    let defs = no_defs();
    // Element width only — the runtime multiplies by the length.
    assert_eq!(
        accelerable_byte_size(
            &custom(array_name(), vec![TypeKind::I32, TypeKind::Int]),
            &defs
        ),
        Some(4)
    );
    assert_eq!(
        accelerable_byte_size(&custom(list_name(), vec![TypeKind::F64]), &defs),
        Some(8)
    );
    assert_eq!(
        accelerable_byte_size(
            &custom(array_name(), vec![TypeKind::Int, TypeKind::Int]),
            &defs
        ),
        Some(8)
    );
}

#[test]
fn byte_size_vector_is_dim_times_component() {
    let defs = no_defs();
    assert_eq!(
        accelerable_byte_size(&custom(VEC3_TYPE_NAME, vec![TypeKind::F32]), &defs),
        Some(12)
    );
    assert_eq!(
        accelerable_byte_size(&custom(VEC4_TYPE_NAME, vec![TypeKind::F32]), &defs),
        Some(16)
    );
}

#[test]
fn byte_size_tuple_is_sum_of_fields() {
    let defs = no_defs();
    assert_eq!(
        accelerable_byte_size(
            &TypeKind::Tuple(vec![
                type_expr_non_null(make_type(TypeKind::I32)),
                type_expr_non_null(make_type(TypeKind::F64)),
            ]),
            &defs
        ),
        Some(12)
    );
}

#[test]
fn byte_size_struct_sums_field_widths_from_the_type_table() {
    let mut defs = no_defs();
    defs.insert(
        "Point".to_string(),
        TypeDefinition::Struct(StructDefinition {
            fields: vec![
                (
                    "x".to_string(),
                    make_type(TypeKind::I32),
                    MemberVisibility::Public,
                ),
                (
                    "y".to_string(),
                    make_type(TypeKind::F64),
                    MemberVisibility::Public,
                ),
            ],
            generics: None,
            traits: vec![ACCELERABLE_TRAIT_NAME.to_string()],
            module: String::new(),
            has_drop: false,
        }),
    );
    assert_eq!(
        accelerable_byte_size(&TypeKind::Custom("Point".to_string(), None), &defs),
        Some(12)
    );
}

#[test]
fn byte_size_string_has_no_device_width() {
    assert_eq!(accelerable_byte_size(&TypeKind::String, &no_defs()), None);
}

#[test]
fn byte_size_follows_scalar_alias_to_its_width() {
    // `type Byte is u8`
    let mut defs = no_defs();
    defs.insert("Byte".to_string(), alias(TypeKind::U8));

    // A bare alias reference sizes as its underlying scalar.
    assert_eq!(
        accelerable_byte_size(&TypeKind::Custom("Byte".to_string(), None), &defs),
        Some(1)
    );
    // An `Array<Byte, N>` reports the alias element width, not `None`.
    assert_eq!(
        accelerable_byte_size(
            &custom(
                array_name(),
                vec![TypeKind::Custom("Byte".to_string(), None), TypeKind::Int],
            ),
            &defs
        ),
        Some(1)
    );
}

#[test]
fn byte_size_follows_alias_chain() {
    // `type A is B`, `type B is u16`
    let mut defs = no_defs();
    defs.insert(
        "A".to_string(),
        alias(TypeKind::Custom("B".to_string(), None)),
    );
    defs.insert("B".to_string(), alias(TypeKind::U16));
    assert_eq!(
        accelerable_byte_size(&TypeKind::Custom("A".to_string(), None), &defs),
        Some(2)
    );
}

#[test]
fn byte_size_follows_alias_to_a_collection() {
    // `type Buf is Array<i32, 4>` — the element width is still what matters.
    let mut defs = no_defs();
    defs.insert(
        "Buf".to_string(),
        alias(custom(array_name(), vec![TypeKind::I32, TypeKind::Int])),
    );
    assert_eq!(
        accelerable_byte_size(&TypeKind::Custom("Buf".to_string(), None), &defs),
        Some(4)
    );
}

#[test]
fn byte_size_alias_cycle_terminates_without_a_width() {
    // A pathological `type A is B`, `type B is A` must not loop forever.
    let mut defs = no_defs();
    defs.insert(
        "A".to_string(),
        alias(TypeKind::Custom("B".to_string(), None)),
    );
    defs.insert(
        "B".to_string(),
        alias(TypeKind::Custom("A".to_string(), None)),
    );
    assert_eq!(
        accelerable_byte_size(&TypeKind::Custom("A".to_string(), None), &defs),
        None
    );
}
