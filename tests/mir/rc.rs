// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::rc::is_field_managed;

fn expr_ty(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(Type::new(kind, Span::default())), false),
        span: Span::default(),
    }
}

#[test]
fn is_field_managed_classifies_heap_types() {
    let int_expr = Box::new(expr_ty(TypeKind::Int));
    assert!(is_field_managed(&TypeKind::String));
    assert!(is_field_managed(&TypeKind::List(int_expr.clone())));
    assert!(is_field_managed(&TypeKind::Custom(
        "MyClass".to_string(),
        None
    )));
    assert!(!is_field_managed(&TypeKind::Int));
    assert!(!is_field_managed(&TypeKind::Boolean));
}

/// A vector element is stored inline in the collection's buffer, so the drop
/// path must not read its bytes as a pointer and release them.
#[test]
fn is_field_managed_excludes_inline_vector_elements() {
    let vec3 = TypeKind::Custom("Vec3".to_string(), Some(vec![expr_ty(TypeKind::F32)]));
    assert!(!is_field_managed(&vec3));
}
