// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the Perceus reference-counting insertion pass.
//!
//! These assert the *exact placement* of `IncRef` / `DecRef` relative to the
//! statement that made them necessary. Placement — not merely presence — is
//! what makes the RC discipline sound: an `IncRef` emitted after the aliasing
//! assignment, or a `DecRef` emitted after the new value overwrote the old
//! one, still runs the right number of times but on the wrong allocation.
//!
//! Two styles are used deliberately:
//! - synthetic single-block bodies, where every local's type and every
//!   statement is chosen to isolate one decision in the pass;
//! - lowered real Miri sources, which prove the synthetic decisions are the
//!   ones the pipeline actually reaches.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::BasicBlockData;
use miri::mir::lowering::lower_function;
use miri::mir::optimization::{count_rc_ops, insert_rc};
use miri::mir::rc::RcClassifier;
use miri::mir::type_facts::{ClassDefinition, GenericDefinition};
use miri::mir::verify::verify_body;
use miri::mir::{
    AggregateKind, Body, ExecutionModel, Local, LocalDecl, Operand, Place, PlaceElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;
use miri::type_checker::context::{AliasDefinition, FieldInfo, TypeDefinition};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn type_expr(kind: TypeKind) -> Expression {
    Expression::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
}

/// The post-normalization spelling of a collection type: `Custom(name, [args])`.
fn collection(name: &str, args: Vec<TypeKind>) -> Type {
    ty(TypeKind::Custom(
        name.to_string(),
        Some(args.into_iter().map(type_expr).collect()),
    ))
}

fn custom(name: &str) -> Type {
    ty(TypeKind::Custom(name.to_string(), None))
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn field(local: usize, index: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Field(index)],
    }
}

fn index(local: usize, index_local: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Index(Local(index_local))],
    }
}

fn deref(local: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Deref],
    }
}

fn assign(dest: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(dest, rvalue),
        span: span(),
    }
}

fn reassign(dest: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Reassign(dest, rvalue),
        span: span(),
    }
}

fn storage_dead(target: Place) -> Statement {
    Statement {
        kind: StatementKind::StorageDead(target),
        span: span(),
    }
}

/// A single-block body whose local `i` has type `local_tys[i]`.
///
/// Local 0 is the return slot and locals `1..=arg_count` are parameters, matching
/// the layout Perceus assumes when it decides which locals it owns.
fn body_with(local_tys: &[Type], arg_count: usize, statements: Vec<Statement>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for local_ty in local_tys {
        body.new_local(LocalDecl::new(local_ty.clone(), span()));
    }
    body.basic_blocks.push(BasicBlockData {
        statements,
        terminator: Some(Terminator {
            kind: TerminatorKind::Return,
            span: span(),
        }),
        is_cleanup: false,
    });
    body
}

/// Run Perceus and return the entry block's statement kinds in order.
fn rc_statements(body: &mut Body) -> Vec<StatementKind> {
    insert_rc(body);
    body.basic_blocks[0]
        .statements
        .iter()
        .map(|stmt| stmt.kind.clone())
        .collect()
}

fn use_copy(source: Place) -> Rvalue {
    Rvalue::Use(Operand::Copy(source))
}

fn use_move(source: Place) -> Rvalue {
    Rvalue::Use(Operand::Move(source))
}

#[test]
fn test_incref_of_source_precedes_the_aliasing_assignment() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        0,
        vec![assign(place(2), use_copy(place(1)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(2), use_copy(place(1))),
        ],
        "IncRef must be emitted before the assignment that creates the alias"
    );
}

#[test]
fn test_copy_of_scalar_local_gets_no_rc_ops() {
    let mut body = body_with(
        &[ty(TypeKind::Void), ty(TypeKind::Int), ty(TypeKind::Int)],
        0,
        vec![assign(place(2), use_copy(place(1)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(2), use_copy(place(1)))],
        "scalar copies must not be reference counted"
    );
}

#[test]
fn test_ref_rvalue_increfs_like_a_copy() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        0,
        vec![assign(place(2), Rvalue::Ref(place(1)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(2), Rvalue::Ref(place(1))),
        ],
        "a Ref rvalue aliases the place and must IncRef it"
    );
}

#[test]
fn test_decref_precedes_storage_dead_of_owned_managed_local() {
    let mut body = body_with(
        &[ty(TypeKind::Void), ty(TypeKind::String)],
        0,
        vec![storage_dead(place(1))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::DecRef(place(1)),
            StatementKind::StorageDead(place(1)),
        ],
        "DecRef must run while the storage is still live"
    );
}

#[test]
fn test_storage_dead_of_parameter_gets_no_decref() {
    // Local _1 is the single parameter: caller-owned, so the callee must not
    // release it at scope exit.
    let mut body = body_with(
        &[ty(TypeKind::Void), ty(TypeKind::String)],
        1,
        vec![storage_dead(place(1))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::StorageDead(place(1))],
        "parameters are borrowed; DecRef would free the caller's allocation"
    );
}

#[test]
fn test_reassign_decrefs_the_old_value_after_increfing_the_new_one() {
    // Order matters for `s = s`: the IncRef of the source must run before the
    // DecRef of the destination, or a self-assignment frees the value it reads.
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        0,
        vec![reassign(place(1), use_copy(place(2)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(2)),
            StatementKind::DecRef(place(1)),
            StatementKind::Reassign(place(1), use_copy(place(2))),
        ],
    );
}

#[test]
fn test_self_reassign_increfs_before_decref() {
    let mut body = body_with(
        &[ty(TypeKind::Void), ty(TypeKind::String)],
        0,
        vec![reassign(place(1), use_copy(place(1)))],
    );

    let statements = rc_statements(&mut body);
    let incref_at = statements
        .iter()
        .position(|kind| matches!(kind, StatementKind::IncRef(_)))
        .expect("self-reassignment must IncRef the source");
    let decref_at = statements
        .iter()
        .position(|kind| matches!(kind, StatementKind::DecRef(_)))
        .expect("self-reassignment must DecRef the old value");

    assert!(
        incref_at < decref_at,
        "IncRef must precede DecRef so `s = s` cannot free its own source: {:?}",
        statements
    );
}

#[test]
fn test_reassign_of_parameter_gets_no_decref() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        1,
        vec![reassign(place(1), use_copy(place(2)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(2)),
            StatementKind::Reassign(place(1), use_copy(place(2))),
        ],
        "a reassigned parameter still holds the caller's reference"
    );
}

#[test]
fn test_move_from_parameter_increfs_the_parameter() {
    // The caller does not IncRef before the call (borrow semantics), so a move
    // out of a parameter must IncRef to balance the destination's StorageDead.
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        1,
        vec![assign(place(2), use_move(place(1)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(2), use_move(place(1))),
        ],
    );
}

#[test]
fn test_move_from_local_gets_no_incref() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        1,
        vec![assign(place(3), use_move(place(2)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(3), use_move(place(2)))],
        "a move between owned locals transfers the reference; no IncRef"
    );
}

#[test]
fn test_cast_of_moved_parameter_increfs_the_parameter() {
    let cast = Rvalue::Cast(Box::new(Operand::Move(place(1))), ty(TypeKind::String));
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        1,
        vec![assign(place(2), cast.clone())],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(2), cast),
        ],
    );
}

#[test]
fn test_cast_of_moved_parameter_to_unmanaged_destination_gets_no_incref() {
    // A cast to `Self` is how a stdlib method forwards its receiver to a
    // `runtime "core" fn` whose parameter is declared `Self`.  `Self` is not a
    // managed type, so the destination never reaches a DecRef; retaining the
    // parameter here would strand the reference and leak the receiver.
    let cast = Rvalue::Cast(Box::new(Operand::Move(place(1))), custom("Self"));
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Point"), custom("Self")],
        1,
        vec![assign(place(2), cast.clone())],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(2), cast)],
        "an unmanaged destination emits no DecRef, so the move must not IncRef"
    );
}

#[test]
fn test_copy_of_managed_struct_field_increfs_the_projected_place() {
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Holder"), ty(TypeKind::String)],
        0,
        vec![assign(place(2), use_copy(field(1, 0)))],
    );
    body.field_types = HashMap::from([("Holder".to_string(), vec![ty(TypeKind::String)])]);

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(field(1, 0)),
            StatementKind::Assign(place(2), use_copy(field(1, 0))),
        ],
        "the IncRef must name the projected field, not the owning struct"
    );
}

/// A single-parameter body returning `_1.0`, where `_1` is `Tagged<arg>` and
/// `Tagged` declares its only field at its type parameter `T`.
///
/// The value lands in the return slot, which the pass never counts among the
/// locals it owns, so only the projected field's own type can call for a retain.
fn return_of_generic_class_field(arg: TypeKind) -> Body {
    let mut body = body_with(
        &[ty(TypeKind::String), collection("Tagged", vec![arg])],
        1,
        vec![assign(place(0), use_copy(field(1, 0)))],
    );
    body.field_types = HashMap::from([(
        "Tagged".to_string(),
        vec![ty(TypeKind::Generic(
            "T".to_string(),
            None,
            miri::ast::types::TypeDeclarationKind::None,
        ))],
    )]);
    body.class_type_params = HashMap::from([("Tagged".to_string(), vec!["T".to_string()])]);
    let definitions = HashMap::from([("Tagged".to_string(), tagged_definition())]);
    body.rc_classifier = RcClassifier::new(Rc::new(definitions));
    body
}

/// `class Tagged<T>` with the one field `value T`, as the program defines it.
fn tagged_definition() -> TypeDefinition {
    let param = ty(TypeKind::Generic(
        "T".to_string(),
        None,
        miri::ast::types::TypeDeclarationKind::None,
    ));
    TypeDefinition::Class(ClassDefinition {
        name: "Tagged".to_string(),
        generics: Some(vec![GenericDefinition {
            name: "T".to_string(),
            constraint: None,
            kind: miri::ast::types::TypeDeclarationKind::None,
        }]),
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![(
            "value".to_string(),
            FieldInfo {
                ty: param,
                mutable: true,
                visibility: miri::ast::MemberVisibility::Public,
                initializer: None,
            },
        )],
        methods: BTreeMap::new(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    })
}

#[test]
fn test_field_declared_at_the_class_parameter_is_retained_at_a_managed_argument() {
    let mut body = return_of_generic_class_field(TypeKind::String);

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(field(1, 0)),
            StatementKind::Assign(place(0), use_copy(field(1, 0))),
        ],
        "a `value T` field of a `Tagged<String>` holds a string and must be retained"
    );
}

#[test]
fn test_field_declared_at_the_class_parameter_is_not_retained_at_a_scalar_argument() {
    let mut body = return_of_generic_class_field(TypeKind::Int);

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(0), use_copy(field(1, 0)))],
        "a `value T` field of a `Tagged<int>` holds a bare integer"
    );
}

#[test]
fn test_copy_of_scalar_struct_field_gets_no_incref() {
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Holder"), ty(TypeKind::Int)],
        0,
        vec![assign(place(2), use_copy(field(1, 0)))],
    );
    body.field_types = HashMap::from([("Holder".to_string(), vec![ty(TypeKind::Int)])]);

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(2), use_copy(field(1, 0)))],
    );
}

#[test]
fn test_unresolvable_field_projection_increfs_when_destination_is_managed() {
    // Enum `Field(i)` types cannot be resolved without knowing the active
    // variant, so the pass falls back to the destination's own managed-ness.
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Shape"), ty(TypeKind::String)],
        0,
        vec![assign(place(2), use_copy(field(1, 0)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(field(1, 0)),
            StatementKind::Assign(place(2), use_copy(field(1, 0))),
        ],
    );
}

#[test]
fn test_cast_of_managed_field_increfs_the_projected_place() {
    let cast = Rvalue::Cast(Box::new(Operand::Copy(field(1, 0))), ty(TypeKind::String));
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Holder"), ty(TypeKind::String)],
        0,
        vec![assign(place(2), cast.clone())],
    );
    body.field_types = HashMap::from([("Holder".to_string(), vec![ty(TypeKind::String)])]);

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(field(1, 0)),
            StatementKind::Assign(place(2), cast),
        ],
    );
}

#[test]
fn test_copy_of_managed_list_element_increfs_the_indexed_place() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            collection("List", vec![TypeKind::String]),
            ty(TypeKind::Int),
            ty(TypeKind::String),
        ],
        0,
        vec![assign(place(3), use_copy(index(1, 2)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(index(1, 2)),
            StatementKind::Assign(place(3), use_copy(index(1, 2))),
        ],
    );
}

/// The whole collection is managed even when its elements are not: a copy of the
/// bare local is judged by the local's own type, a copy through `Index` by the
/// element type the projection reaches.
#[test]
fn test_copy_of_scalar_list_increfs_the_list_but_not_its_element() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            collection("List", vec![TypeKind::Int]),
            ty(TypeKind::Int),
            ty(TypeKind::Int),
            collection("List", vec![TypeKind::Int]),
        ],
        0,
        vec![
            assign(place(4), use_copy(place(1))),
            assign(place(3), use_copy(index(1, 2))),
        ],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(4), use_copy(place(1))),
            StatementKind::Assign(place(3), use_copy(index(1, 2))),
        ],
    );
}

#[test]
fn test_copy_of_scalar_list_element_gets_no_incref() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            collection("List", vec![TypeKind::Int]),
            ty(TypeKind::Int),
            ty(TypeKind::Int),
        ],
        0,
        vec![assign(place(3), use_copy(index(1, 2)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(3), use_copy(index(1, 2)))],
    );
}

#[test]
fn test_copy_through_deref_gets_no_incref() {
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::String),
        ],
        0,
        vec![assign(place(2), use_copy(deref(1)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(2), use_copy(deref(1)))],
        "a Deref projection escapes the tracked ownership tree"
    );
}

#[test]
fn test_aggregate_increfs_each_managed_operand_in_order() {
    let aggregate = Rvalue::Aggregate(
        AggregateKind::List,
        vec![
            Operand::Copy(place(1)),
            Operand::Copy(place(2)),
            Operand::Copy(place(3)),
        ],
    );
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            ty(TypeKind::String),
            ty(TypeKind::Int),
            ty(TypeKind::String),
            collection("List", vec![TypeKind::String]),
        ],
        0,
        vec![assign(place(4), aggregate.clone())],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::IncRef(place(3)),
            StatementKind::Assign(place(4), aggregate),
        ],
        "the aggregate takes a reference to each managed element only"
    );
}

#[test]
fn test_generic_type_parameter_place_gets_no_incref() {
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("T"), custom("T")],
        0,
        vec![assign(place(2), use_copy(place(1)))],
    );
    body.type_params.insert("T".to_string());

    assert_eq!(
        rc_statements(&mut body),
        vec![StatementKind::Assign(place(2), use_copy(place(1)))],
        "an unresolved generic placeholder has no known representation to count"
    );
}

#[test]
fn test_monomorphized_class_place_is_increfd() {
    // Same shape as the generic case, but after monomorphization the local's
    // type names a concrete class rather than a type parameter in scope.
    let mut body = body_with(
        &[
            ty(TypeKind::Void),
            custom("Box__String"),
            custom("Box__String"),
        ],
        0,
        vec![assign(place(2), use_copy(place(1)))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(2), use_copy(place(1))),
        ],
    );
}

#[test]
fn test_alias_to_scalar_gets_no_rc_ops() {
    // A local spelled with an alias to a primitive (`type Meters is int`, as
    // `Custom("Meters")`) has no allocation behind it: the classifier reads
    // the alias through the program's type table.
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Meters"), custom("Meters")],
        0,
        vec![assign(place(2), use_copy(place(1))), storage_dead(place(2))],
    );
    let meters = TypeDefinition::Alias(AliasDefinition {
        template: ty(TypeKind::Int),
        generics: None,
    });
    let definitions = HashMap::from([("Meters".to_string(), meters)]);
    body.rc_classifier = RcClassifier::new(Rc::new(definitions));

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::Assign(place(2), use_copy(place(1))),
            StatementKind::StorageDead(place(2)),
        ],
        "a name with no heap object behind it is never reference counted"
    );
}

#[test]
fn test_auto_copy_struct_is_reference_counted() {
    // An auto-copy struct is copied bitwise on assignment, but it is still a
    // heap-allocated aggregate, so its storage must be released. Excluding it
    // from RC left it allocated with nothing to free it.
    let mut body = body_with(
        &[ty(TypeKind::Void), custom("Point"), custom("Point")],
        0,
        vec![assign(place(2), use_copy(place(1))), storage_dead(place(2))],
    );

    assert_eq!(
        rc_statements(&mut body),
        vec![
            StatementKind::IncRef(place(1)),
            StatementKind::Assign(place(2), use_copy(place(1))),
            StatementKind::DecRef(place(2)),
            StatementKind::StorageDead(place(2)),
        ],
        "an aggregate that is heap-allocated must also be released"
    );
}

#[test]
fn test_body_without_managed_locals_is_left_unchanged() {
    let statements = vec![assign(place(2), use_copy(place(1))), storage_dead(place(2))];
    let mut body = body_with(
        &[ty(TypeKind::Void), ty(TypeKind::Int), ty(TypeKind::Int)],
        0,
        statements.clone(),
    );

    let expected: Vec<StatementKind> = statements.into_iter().map(|s| s.kind).collect();
    assert_eq!(rc_statements(&mut body), expected);
}

/// Lower `source` and run Perceus on the function named `name`.
fn lowered_with_rc(source: &str, name: &str) -> Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend should succeed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| match &stmt.node {
            AstStatementKind::FunctionDeclaration(decl) => decl.name == name,
            _ => false,
        })
        .unwrap_or_else(|| panic!("function '{}' not found", name));

    let (mut body, _) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("Lowering failed");
    insert_rc(&mut body);
    body
}

fn all_statements(body: &Body) -> Vec<StatementKind> {
    body.basic_blocks
        .iter()
        .flat_map(|block| block.statements.iter().map(|stmt| stmt.kind.clone()))
        .collect()
}

/// The body's RC and storage statements in order, dropping the assignments in
/// between so a placement assertion does not depend on lowering's temp numbering.
fn rc_and_storage_statements(body: &Body) -> Vec<StatementKind> {
    all_statements(body)
        .into_iter()
        .filter(|kind| {
            matches!(
                kind,
                StatementKind::IncRef(_)
                    | StatementKind::DecRef(_)
                    | StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
            )
        })
        .collect()
}

fn count_rc(body: &Body) -> (usize, usize) {
    let mut increfs = 0;
    let mut decrefs = 0;
    for kind in all_statements(body) {
        match kind {
            StatementKind::IncRef(_) => increfs += 1,
            StatementKind::DecRef(_) => decrefs += 1,
            _ => {}
        }
    }
    (increfs, decrefs)
}

/// True when every `DecRef(p)` Perceus emitted sits immediately before the
/// statement that consumes `p`: the `StorageDead(p)` ending its scope, or the
/// `Reassign(p, _)` overwriting it.
fn every_decref_guards_its_consumer(body: &Body) -> bool {
    body.basic_blocks.iter().all(|block| {
        block
            .statements
            .iter()
            .enumerate()
            .filter_map(|(i, stmt)| match &stmt.kind {
                StatementKind::DecRef(target) => Some((i, target)),
                _ => None,
            })
            .all(
                |(i, target)| match block.statements.get(i + 1).map(|s| &s.kind) {
                    Some(StatementKind::StorageDead(next)) => next == target,
                    Some(StatementKind::Reassign(next, _)) => next == target,
                    _ => false,
                },
            )
    })
}

#[test]
fn test_lowered_field_copy_increfs_the_projected_field() {
    let source = r#"
struct Holder
    name String

fn read_name(h Holder) String:
    return h.name
"#;
    let body = lowered_with_rc(source, "read_name");
    let statements = all_statements(&body);

    assert!(
        statements.iter().any(|kind| matches!(
            kind,
            StatementKind::IncRef(p)
                if p.projection.iter().any(|e| matches!(e, PlaceElem::Field(_)))
        )),
        "copying a managed field out of a struct must IncRef the field place: {:?}",
        statements
    );
}

#[test]
fn test_lowered_alias_to_tuple_is_still_released() {
    // A declared type reaches lowering spelled as written, so an alias arrives
    // as its own name with nothing to say the value behind it is heap
    // allocated. Resolving the alias is what lets the RC pass see the tuple;
    // without it the block the aggregate allocated is never released.
    // A tuple of scalars is held as its bytes and released by nobody, so the
    // tuple here holds a string.
    let source = r#"
type Pair is (int, String)

fn probe()
    let p Pair = (1, "one")
"#;
    let body = lowered_with_rc(source, "probe");

    assert!(
        all_statements(&body)
            .iter()
            .any(|kind| matches!(kind, StatementKind::DecRef(_))),
        "the aliased tuple must be released: {:?}",
        all_statements(&body)
    );
}

#[test]
fn test_lowered_alias_to_primitive_is_not_reference_counted() {
    // The complement, and the reason resolving the alias cannot be skipped in
    // either direction: an alias to a bare integer allocates nothing, so
    // emitting RC ops for it would release a value that was never a heap
    // object.
    let source = r#"
type Meters is int

fn probe()
    let m Meters = 5
"#;
    let body = lowered_with_rc(source, "probe");

    assert!(
        !all_statements(&body)
            .iter()
            .any(|kind| matches!(kind, StatementKind::DecRef(_))),
        "an aliased integer must not be released: {:?}",
        all_statements(&body)
    );
}

#[test]
fn test_lowered_string_reassignment_decrefs_the_replaced_value() {
    let source = r#"
fn revive() String:
    var s = "first"
    s = "second"
    return s
"#;
    let body = lowered_with_rc(source, "revive");
    let statements = all_statements(&body);

    let reassign_at = statements
        .iter()
        .position(|kind| matches!(kind, StatementKind::Reassign(..)))
        .expect("lowering should emit Reassign for an overwrite");
    assert!(
        statements[..reassign_at]
            .iter()
            .any(|kind| matches!(kind, StatementKind::DecRef(_))),
        "the replaced string must be released before the overwrite: {:?}",
        statements
    );
}

#[test]
fn test_lowered_resource_type_gets_balanced_rc_ops() {
    let source = r#"
class Conn
    handle int
    fn drop(self)
        return

fn use_conn(conn Conn) int:
    let alias = conn
    return alias.handle
"#;
    let body = lowered_with_rc(source, "use_conn");
    let (increfs, decrefs) = count_rc(&body);

    assert!(
        body.has_drop_types.contains("Conn"),
        "a struct with `fn drop` must be recorded as a resource type"
    );
    assert!(
        increfs > 0 && decrefs > 0,
        "aliasing a resource must be reference counted: incref={}, decref={}",
        increfs,
        decrefs
    );
    assert_eq!(
        increfs, decrefs,
        "resource aliasing must leave IncRef and DecRef balanced"
    );
}

/// A generic class's field declared at its parameter (`value T`) is read at the
/// instance's type arguments, so out of a `Box<String>` it is a string. The
/// instance is released through the drop function of its own instantiation,
/// which releases that string, so the value read out takes its own reference
/// before the instance goes.
#[test]
fn test_generic_field_read_is_increfd_and_instance_is_released_once() {
    let source = r#"
class Box<T>
    value T

    fn init(v T)
        self.value = v

fn unwrap_box() String:
    let b = Box<String>("hello")
    return b.value
"#;
    let body = lowered_with_rc(source, "unwrap_box");

    assert_eq!(
        rc_and_storage_statements(&body),
        vec![
            StatementKind::StorageLive(place(1)),
            StatementKind::IncRef(Place {
                local: Local(1),
                projection: vec![PlaceElem::Field(0)],
            }),
            StatementKind::DecRef(place(1)),
            StatementKind::StorageDead(place(1)),
        ],
        "the field read out is retained and the instance is released exactly once"
    );
}

#[test]
fn test_lowered_bodies_pass_rc_verification() {
    let sources = [
        (
            "read_name",
            r#"
struct Holder
    name String

fn read_name(h Holder) String:
    return h.name
"#,
        ),
        (
            "revive",
            r#"
fn revive() String:
    var s = "first"
    s = "second"
    return s
"#,
        ),
        (
            "use_conn",
            r#"
class Conn
    handle int
    fn drop(self)
        return

fn use_conn(conn Conn) int:
    let alias = conn
    return alias.handle
"#,
        ),
    ];

    for (name, source) in sources {
        let body = lowered_with_rc(source, name);
        let violations = verify_body(&body);
        assert!(
            violations.is_empty(),
            "RC violations in '{}' after Perceus: {:?}",
            name,
            violations
        );
        assert!(
            every_decref_guards_its_consumer(&body),
            "a DecRef in '{}' is not adjacent to the StorageDead or Reassign it guards: {:?}",
            name,
            all_statements(&body)
        );
    }
}

/// A match subject is copied, never moved, out of the local it came from.
///
/// `release_subject_temps` releases both the subject temp and the local that
/// backed it. That is only balanced if reading the subject retained it: a move
/// would leave one allocation with two `DecRef`s against it, freeing a live
/// value the moment the match ended.
#[test]
fn test_match_subject_from_named_local_is_retained() {
    let body = lowered_with_rc(
        r#"
fn probe(seed String) int
    let value = Some(seed + "!")
    match value
        Some(text): text.length()
        None: 0
"#,
        "probe",
    );

    let statements = all_statements(&body);
    let subject_assign = statements
        .iter()
        .position(|statement| match statement {
            StatementKind::Assign(_, Rvalue::Use(Operand::Copy(place)))
            | StatementKind::Assign(_, Rvalue::Use(Operand::Move(place))) => {
                place.projection.is_empty() && place.local == Local(2)
            }
            _ => false,
        })
        .expect("the subject local should be read into the match temp");

    match &statements[subject_assign] {
        StatementKind::Assign(_, Rvalue::Use(Operand::Copy(_))) => {}
        other => panic!("match subject must be copied, not moved: {:?}", other),
    }

    assert!(
        matches!(
            &statements[subject_assign - 1],
            StatementKind::IncRef(place) if place.local == Local(2) && place.projection.is_empty()
        ),
        "the copy into the match temp must be retained, got {:?}",
        &statements[subject_assign - 1]
    );
}

/// A store through a field projection hands its reference to the object being
/// written, so the source has to be retained before it. Without the retain the
/// field and the source share one reference that both release, and the field is
/// left holding freed memory — the shape a `Move` into the field used to emit.
#[test]
fn a_store_into_a_managed_field_retains_the_source() {
    let body = lowered_with_rc(
        "
use system.collections.list

struct Holder
    data [int]

fn main()
    var src = List([1, 2])
    var holder = Holder(List<int>())
    holder.data = src
",
        "main",
    );

    let statements = all_statements(&body);
    let store = statements
        .iter()
        .position(|kind| {
            matches!(
                kind,
                StatementKind::Reassign(place, Rvalue::Use(Operand::Copy(_)))
                    if matches!(place.projection.last(), Some(PlaceElem::Field(_)))
            )
        })
        .unwrap_or_else(|| panic!("no copy into a field found in {:?}", statements));

    let Some(StatementKind::Reassign(field, _)) = statements.get(store) else {
        unreachable!("the position above matched a Reassign")
    };

    assert!(
        statements[..store].iter().any(
            |kind| matches!(kind, StatementKind::IncRef(place) if place.projection.is_empty())
        ),
        "the value stored into the field must be retained first, got {:?}",
        &statements[..=store]
    );
    assert!(
        statements[..store].iter().any(
            |kind| matches!(kind, StatementKind::DecRef(place) if place.projection == field.projection)
        ),
        "the field's old value must be released before the store, got {:?}",
        &statements[..=store]
    );
}

#[test]
fn test_a_string_picked_out_of_a_tuple_by_a_value_is_retained() {
    let source = r#"
fn probe() String
    let x = ("a" + "b", "c" + "d")
    var i = 1
    let y = x[i]
    return y
"#;
    let body = lowered_with_rc(source, "probe");
    assert!(
        all_statements(&body).iter().any(|kind| matches!(
            kind,
            StatementKind::IncRef(p)
                if p.projection.iter().any(|e| matches!(e, PlaceElem::Index(_)))
        )),
        "the element read out of the tuple must be retained: {:?}",
        all_statements(&body)
    );
}

/// The local of `body` named `name`.
fn local_named(body: &Body, name: &str) -> Local {
    let index = body
        .local_decls
        .iter()
        .position(|decl| decl.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no local named `{name}`"));
    Local(index)
}

/// The local `body` defines by copying something out of `owner` through
/// `step`.
fn read_out_of(body: &Body, owner: Local, step: fn(&PlaceElem) -> bool) -> Local {
    all_statements(body)
        .into_iter()
        .find_map(|kind| match kind {
            StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(source)))
                if source.local == owner && source.projection.first().is_some_and(step) =>
            {
                Some(dest.local)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("nothing is read out of {owner}"))
}

/// Whether the read defining `local` is retained — an `IncRef` of the place
/// it copies right before it — and how many times `local` is released.
fn retain_and_releases(body: &Body, local: Local) -> (bool, usize) {
    let statements = all_statements(body);
    let retained = statements.windows(2).any(|pair| match pair {
        [StatementKind::IncRef(retained), StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(source)))] => {
            dest.local == local && retained == source
        }
        _ => false,
    });
    (retained, count_rc_ops(body, local).1)
}

fn is_index(elem: &PlaceElem) -> bool {
    matches!(elem, PlaceElem::Index(_))
}

fn is_field(elem: &PlaceElem) -> bool {
    matches!(elem, PlaceElem::Field(_))
}

const BOX_CLASS: &str = r#"
use system.collections.list

class Box
    var v int

    fn init(v int)
        self.v = v
"#;

/// A loop variable over a list the body owns, which the loop leaves alone,
/// borrows each element: it is neither retained nor released.
#[test]
fn test_a_loop_variable_over_a_local_list_borrows_its_element() {
    let source = format!(
        "{BOX_CLASS}
fn total() int
    let xs = List([Box(1), Box(2)])
    var t = 0
    for s in xs
        t = t + s.v
    t
"
    );
    let body = lowered_with_rc(&source, "total");
    let s = local_named(&body, "s");
    assert_eq!(count_rc_ops(&body, s), (0, 0));
    assert_eq!(retain_and_releases(&body, s), (false, 0));
    assert!(body.borrowed_locals.contains_key(&s));
}

/// A match subject read out of a list the body owns borrows the element.
#[test]
fn test_a_match_subject_read_out_of_a_local_list_borrows_it() {
    let source = r#"
use system.collections.list

enum Instr
    Add(int)
    Halt

fn run() int
    let code = List([Instr.Add(3), Instr.Halt])
    var acc = 0
    var pc = 0
    while pc < 2
        match code[pc]
            Instr.Add(v): acc = acc + v
            Instr.Halt: acc = acc + 1
        pc += 1
    acc
"#;
    let body = lowered_with_rc(source, "run");
    let subject = read_out_of(&body, local_named(&body, "code"), is_index);
    assert_eq!(count_rc_ops(&body, subject), (0, 0));
    assert_eq!(retain_and_releases(&body, subject), (false, 0));
}

/// A list a parameter holds may be emptied through another name — here
/// `h.wipe()` clears it — so an element read out of it keeps its reference.
#[test]
fn test_an_element_of_a_parameter_list_keeps_its_reference() {
    let source = format!(
        "{BOX_CLASS}
class Holder
    var items [Box]

    fn init(items [Box])
        self.items = items

    fn wipe()
        self.items.clear()

fn probe(h Holder, xs [Box]) int
    let x = xs[0]
    h.wipe()
    x.v
"
    );
    let body = lowered_with_rc(&source, "probe");
    assert_eq!(
        retain_and_releases(&body, local_named(&body, "x")),
        (true, 1)
    );
    assert!(body.borrowed_locals.is_empty());
}

/// A method can clear a class field it is never shown naming, so a value read
/// out of one keeps its reference.
#[test]
fn test_a_value_read_out_of_a_class_field_keeps_its_reference() {
    let source = r#"
class Node
    var left Node?
    var v int

    fn init(left Node?, v int)
        self.left = left
        self.v = v

    fn cut()
        self.left = None

fn probe() int
    let node = Node(Node(None, 7), 1)
    match node.left
        Some(l)
            node.cut()
            l.v
        None: 0
"#;
    let body = lowered_with_rc(source, "probe");
    let subject = read_out_of(&body, local_named(&body, "node"), is_field);
    assert_eq!(retain_and_releases(&body, subject), (true, 1));
}

/// `row.push` asks whether `row` is shared before it writes; a borrowed `row`
/// would answer no and write into `grid`'s element, so it keeps its reference.
#[test]
fn test_an_element_written_through_keeps_its_reference() {
    let source = r#"
use system.collections.list

fn grow() int
    var grid = List([List([1, 2]), List([3])])
    let row = grid[0]
    row.push(9)
    row.length()
"#;
    let body = lowered_with_rc(source, "grow");
    assert_eq!(
        retain_and_releases(&body, local_named(&body, "row")),
        (true, 1)
    );
}

/// A row lent to a function borrows `grid`'s element: the callee writes the
/// copy it binds of its parameter on entry, which takes a reference of its
/// own, so the write copies and `grid` keeps its elements.
#[test]
fn test_an_element_lent_to_a_function_that_writes_it_borrows_it() {
    let source = r#"
use system.collections.list

fn grow(r [int]) int
    r.push(9)
    r.length()

fn t() int
    var grid = List([List([1, 2]), List([3])])
    var n = 0
    for row in grid
        n = n + grow(row)
    n
"#;
    let body = lowered_with_rc(source, "t");
    let row = local_named(&body, "row");
    assert_eq!(retain_and_releases(&body, row), (false, 0));
    assert!(body.borrowed_locals.contains_key(&row));
}

/// `for row in grid` lends each row to `sum`, a function that only reads it:
/// the loop variable borrows the row and nothing is retained per iteration.
#[test]
fn test_a_loop_variable_lent_to_a_function_borrows_its_element() {
    let source = r#"
use system.collections.list

fn sum(xs [int]) int
    var t = 0
    for x in xs
        t = t + x
    t

fn run() int
    var grid = List([List([1, 2]), List([3, 4]), List([5, 6])])
    var total = 0
    for row in grid
        total = total + sum(row)
    total
"#;
    let body = lowered_with_rc(source, "run");
    let row = local_named(&body, "row");
    assert_eq!(count_rc_ops(&body, row), (0, 0));
    assert_eq!(retain_and_releases(&body, row), (false, 0));
    assert!(body.borrowed_locals.contains_key(&row));
}

/// A method's receiver is not copied on entry: it relies on the caller's
/// copy-on-write, and a check that it arrived unique would read a borrowed
/// row's count as its owner's. A row a method is called on keeps its
/// reference, even for a method that only reads it.
#[test]
fn test_an_element_a_method_is_called_on_keeps_its_reference() {
    let source = r#"
use system.collections.list

fn t() bool
    var grid = List([List([1, 2]), List([3])])
    let row = grid[0]
    row.contains(2)
"#;
    let body = lowered_with_rc(source, "t");
    assert_eq!(
        retain_and_releases(&body, local_named(&body, "row")),
        (true, 1)
    );
}
