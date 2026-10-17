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

/// A function value is a closure allocation of its own, so a field or element
/// holding one must release it.
#[test]
fn is_field_managed_includes_function_values() {
    use miri::ast::types::FunctionTypeData;
    let fn_kind = TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    }));
    assert!(is_field_managed(&fn_kind));
}

/// A vector element is stored inline in the collection's buffer, so the drop
/// path must not read its bytes as a pointer and release them.
#[test]
fn is_field_managed_excludes_inline_vector_elements() {
    let vec3 = TypeKind::Custom("Vec3".to_string(), Some(vec![expr_ty(TypeKind::F32)]));
    assert!(!is_field_managed(&vec3));
}

/// Declares the types `counted` asks about, each spelled as a parameter of
/// `probe`.
const COUNTED_DECLARATIONS: &str = "
struct P
    x int

struct Pair<A, B>
    first A
    second B

class Node
    var value int

class V
    var value int

type Meters is int

fn probe(p P, ints Pair<int, float>, mixed Pair<String, int>, node Node, maybe_int int?, maybe_node Node?, pair (int, int), text String, whole int, meters Meters, v V)
    return

fn main()
    return
";

/// Whether the `probe` parameter `parameter` is reference counted in a body
/// leaving `open` type parameters open.
fn counted(parameter: &str, open: &[&str]) -> bool {
    use miri::ast::statement::StatementKind;
    use miri::mir::rc::RcClassifier;
    use std::collections::HashSet;
    use std::rc::Rc;
    let result = crate::type_checker::utils::type_checker_result(COUNTED_DECLARATIONS);
    let declared = result
        .ast
        .body
        .iter()
        .find_map(|statement| match &statement.node {
            StatementKind::FunctionDeclaration(function) if function.name == "probe" => function
                .params
                .iter()
                .find(|param| param.name == parameter)
                .map(|param| match &param.typ.node {
                    ExpressionKind::Type(ty, false) => (**ty).clone(),
                    ExpressionKind::Type(ty, true) => {
                        Type::new(TypeKind::Option(ty.clone()), ty.span)
                    }
                    other => panic!("parameter type is not a type: {other:?}"),
                }),
            _ => None,
        })
        .expect("`probe` declares the parameter");
    let classifier = RcClassifier::new(Rc::new(result.type_checker.type_definitions().clone()));
    let open: HashSet<String> = open.iter().map(|name| name.to_string()).collect();
    classifier.is_counted(&declared.kind, &open)
}

/// Every value held in an allocation of its own is counted, a bitwise value
/// included: structs, tuples and optionals are still built on the heap.
#[test]
fn rc_classifier_counts_every_heap_value() {
    for parameter in [
        "p",
        "ints",
        "mixed",
        "node",
        "maybe_int",
        "maybe_node",
        "pair",
        "text",
        "v",
    ] {
        assert!(counted(parameter, &[]), "`{parameter}` should be counted");
    }
}

/// A scalar, an alias spelling one, and a type parameter left open — even one
/// named like a type the program defines — hold no count.
#[test]
fn rc_classifier_never_counts_scalars_or_open_parameters() {
    assert!(!counted("whole", &[]));
    assert!(!counted("meters", &[]));
    assert!(!counted("v", &["V"]));
}

/// `void`, and `Self` before substitution, hold no count of their own.
#[test]
fn rc_classifier_never_counts_void_or_self() {
    use miri::mir::rc::RcClassifier;
    use std::collections::HashSet;
    let classifier = RcClassifier::default();
    let open = HashSet::new();
    assert!(!classifier.is_counted(&TypeKind::Void, &open));
    assert!(!classifier.is_counted(&TypeKind::Custom("Self".to_string(), None), &open));
}
