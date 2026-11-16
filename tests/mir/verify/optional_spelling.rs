// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A lowered body spells the optional one way: `T?`. The generic spelling
//! `Option<T>` names the same type, and every later stage lays it out, counts
//! it and releases it only under the first spelling, so a body still holding
//! the second is refused.
//!
//! Each fixture injects the generic spelling where a body can carry a type:
//! a local, a value it builds, a virtual call's parameter. The same shapes
//! written as `T?` are accepted, so a pass is distinguishable from a verifier
//! that reports nothing.

use miri::ast::type_arguments::type_argument;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::symbol::{ClosureKind, Symbol};
use miri::mir::verify::verify_optional_spelling;
use miri::mir::{
    AggregateKind, BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Place, Rvalue,
    Statement, StatementKind,
};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::new(0, 0))
}

/// `Option<int>`, the optional under its generic spelling.
fn generic_optional() -> TypeKind {
    TypeKind::Custom(
        "Option".to_string(),
        Some(vec![type_argument(ty(TypeKind::Int))]),
    )
}

/// `int?`, the optional under its one lowered spelling.
fn sugared_optional() -> TypeKind {
    TypeKind::Option(Box::new(ty(TypeKind::Int)))
}

/// A body returning nothing and declaring one more local at `kind`.
fn body_declaring(kind: TypeKind) -> Body {
    let mut body = Body::new(0, Span::new(0, 0), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::new(0, 0)));
    body.new_local(LocalDecl::new(ty(kind), Span::new(0, 0)));
    body
}

/// A body building an optional at `optional` into its local `_1`.
fn body_building_optional(optional: TypeKind) -> Body {
    let mut body = body_declaring(sugared_optional());
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Aggregate(AggregateKind::Option(ty(optional)), Vec::new()),
        ),
        span: Span::new(0, 0),
    });
    body.basic_blocks.push(block);
    body
}

#[test]
fn a_local_at_the_generic_spelling_is_reported() {
    let found = verify_optional_spelling(&body_declaring(generic_optional()), "Option");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].to_string().contains("`Option<int>`"),
        "{}",
        found[0]
    );
}

#[test]
fn a_generic_spelling_nested_in_a_list_is_reported() {
    let list = TypeKind::List(Box::new(type_argument(ty(generic_optional()))));
    assert_eq!(
        verify_optional_spelling(&body_declaring(list), "Option").len(),
        1
    );
}

#[test]
fn a_generic_spelling_inside_a_sugared_optional_is_reported() {
    let nested = TypeKind::Option(Box::new(ty(generic_optional())));
    assert_eq!(
        verify_optional_spelling(&body_declaring(nested), "Option").len(),
        1
    );
}

#[test]
fn an_optional_built_at_the_generic_spelling_is_reported() {
    let found = verify_optional_spelling(&body_building_optional(generic_optional()), "Option");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].to_string().contains("builds a value at"));
}

#[test]
fn an_optional_built_at_the_sugared_spelling_is_not_reported() {
    assert!(
        verify_optional_spelling(&body_building_optional(sugared_optional()), "Option").is_empty()
    );
}

#[test]
fn a_local_at_the_sugared_spelling_is_not_reported() {
    let nested = TypeKind::Option(Box::new(ty(sugared_optional())));
    assert!(verify_optional_spelling(&body_declaring(nested), "Option").is_empty());
}

#[test]
fn a_class_of_another_name_is_not_reported() {
    let class = TypeKind::Custom(
        "Maybe".to_string(),
        Some(vec![type_argument(ty(TypeKind::Int))]),
    );
    assert!(verify_optional_spelling(&body_declaring(class), "Option").is_empty());
}

#[test]
fn a_closure_built_at_a_signature_spelling_the_generic_name_is_reported() {
    let signature = TypeKind::Function(Box::new(miri::ast::types::FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: Some(Box::new(miri::ast::factory::type_expr_non_null(ty(
            generic_optional(),
        )))),
    }));
    let mut body = body_declaring(TypeKind::Int);
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Aggregate(
                AggregateKind::Closure(Symbol::closure(ClosureKind::Lambda, 0, &[]), ty(signature)),
                Vec::new(),
            ),
        ),
        span: Span::new(0, 0),
    });
    body.basic_blocks.push(block);
    let found = verify_optional_spelling(&body, "Option");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].to_string().contains("builds a value at"));
}

/// A body building the variant `variant` of the enum `enum_name` into `_1`.
fn body_building_variant(enum_name: &str, variant: &str) -> Body {
    let mut body = body_declaring(sugared_optional());
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Aggregate(
                AggregateKind::Enum(enum_name.into(), variant.into()),
                Vec::new(),
            ),
        ),
        span: Span::new(0, 0),
    });
    body.basic_blocks.push(block);
    body
}

#[test]
fn a_variant_of_the_optional_built_as_an_enum_is_reported() {
    let found = verify_optional_spelling(&body_building_variant("Option", "None"), "Option");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0]
        .to_string()
        .contains("builds the variant `Option.None`"));
}

#[test]
fn a_variant_of_another_enum_is_not_reported() {
    let found = verify_optional_spelling(&body_building_variant("Shape", "None"), "Option");
    assert!(found.is_empty(), "{found:?}");
}

/// A body assigning `rvalue` to its local `_1`.
fn body_assigning(rvalue: Rvalue) -> Body {
    let mut body = body_declaring(sugared_optional());
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(Place::new(Local(1)), rvalue),
        span: Span::new(0, 0),
    });
    body.basic_blocks.push(block);
    body
}

/// A body whose one block ends in `kind`.
fn body_ending_in(kind: miri::mir::TerminatorKind) -> Body {
    let mut body = body_declaring(sugared_optional());
    let mut block = BasicBlockData::new(None);
    block.terminator = Some(miri::mir::Terminator::new(kind, Span::new(0, 0)));
    body.basic_blocks.push(block);
    body
}

fn reported_once(body: &Body) {
    let found = verify_optional_spelling(body, "Option");
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn an_instance_at_the_generic_spelling_is_reported() {
    let mut body = body_declaring(sugared_optional());
    body.generics.instance = Some(ty(generic_optional()));
    reported_once(&body);
}

#[test]
fn a_cast_to_the_generic_spelling_is_reported() {
    let operand = miri::mir::Operand::Copy(Place::new(Local(1)));
    reported_once(&body_assigning(Rvalue::Cast(
        Box::new(operand),
        ty(generic_optional()),
    )));
}

#[test]
fn each_aggregate_built_at_the_generic_spelling_is_reported() {
    for kind in [
        AggregateKind::Tuple(ty(generic_optional())),
        AggregateKind::Struct(ty(generic_optional())),
        AggregateKind::Class(ty(generic_optional())),
    ] {
        reported_once(&body_assigning(Rvalue::Aggregate(kind, Vec::new())));
    }
}

#[test]
fn a_constant_at_the_generic_spelling_is_reported() {
    let constant = miri::mir::Operand::Constant(Box::new(miri::mir::Constant {
        span: Span::new(0, 0),
        ty: ty(generic_optional()),
        literal: miri::ast::literal::Literal::None,
    }));
    reported_once(&body_assigning(Rvalue::Use(constant)));
}

#[test]
fn a_virtual_call_passing_at_the_generic_spelling_is_reported() {
    reported_once(&body_ending_in(miri::mir::TerminatorKind::VirtualCall {
        vtable_slot: 0,
        args: Vec::new(),
        arg_modes: Vec::new(),
        param_types: vec![Some(ty(generic_optional()))],
        destination: Place::new(Local(1)),
        target: None,
    }));
}

#[test]
fn a_direct_call_passing_a_constant_at_the_generic_spelling_is_reported() {
    let constant = miri::mir::Operand::Constant(Box::new(miri::mir::Constant {
        span: Span::new(0, 0),
        ty: ty(generic_optional()),
        literal: miri::ast::literal::Literal::None,
    }));
    let callee = miri::mir::Operand::Constant(Box::new(miri::mir::Constant {
        span: Span::new(0, 0),
        ty: ty(TypeKind::Int),
        literal: miri::ast::literal::Literal::None,
    }));
    reported_once(&body_ending_in(miri::mir::TerminatorKind::Call {
        func: callee,
        args: vec![constant],
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        flags: Default::default(),
        destination: Place::new(Local(1)),
        target: None,
    }));
}

#[test]
fn the_generic_spelling_of_another_optional_identity_is_not_reported() {
    let found = verify_optional_spelling(&body_declaring(generic_optional()), "Maybe");
    assert!(found.is_empty(), "{found:?}");
}
