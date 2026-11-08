// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::expression::ExpressionKind;
use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::rc::is_slot_counted;
use miri::type_checker::value_class::LayoutScope;
use std::collections::HashMap;

fn expr_ty(kind: TypeKind) -> TypeArgument {
    TypeArgument {
        id: 0,
        node: TypeArgKind::Type(Box::new(Type::new(kind, Span::default())), false),
        span: Span::default(),
    }
}

#[test]
fn a_slot_holding_a_heap_value_is_counted() {
    let definitions = HashMap::new();
    let scope = LayoutScope::program(&definitions);
    let int_expr = Box::new(expr_ty(TypeKind::Int));
    assert!(is_slot_counted(&TypeKind::String, scope));
    assert!(is_slot_counted(&TypeKind::List(int_expr.clone()), scope));
    assert!(is_slot_counted(
        &TypeKind::Custom("MyClass".to_string(), None),
        scope
    ));
    assert!(!is_slot_counted(&TypeKind::Int, scope));
    assert!(!is_slot_counted(&TypeKind::Boolean, scope));
}

/// A function value is a closure allocation of its own, so a field or element
/// holding one must release it.
#[test]
fn a_slot_holding_a_function_value_is_counted() {
    use miri::ast::types::FunctionTypeData;
    let fn_kind = TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    }));
    let definitions = HashMap::new();
    assert!(is_slot_counted(
        &fn_kind,
        LayoutScope::program(&definitions)
    ));
}

/// A vector is held as its bytes, inline in the collection or aggregate
/// holding it, so the drop path must not read its bytes as a pointer and
/// release them.
#[test]
fn a_vector_held_as_its_bytes_is_no_counted_element_or_slot() {
    let definitions = HashMap::new();
    let scope = LayoutScope::program(&definitions);
    let vec3 = TypeKind::Custom("Vec3".to_string(), Some(vec![expr_ty(TypeKind::F32)]));
    assert!(!is_slot_counted(&vec3, scope));
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

/// Every value held in an allocation of its own is counted, an optional of a
/// reference included: it is that reference's nullable pointer.
#[test]
fn rc_classifier_counts_every_heap_value() {
    for parameter in ["mixed", "node", "maybe_node", "text", "v"] {
        assert!(counted(parameter, &[]), "`{parameter}` should be counted");
    }
}

/// A struct, tuple or optional of scalars — an instantiation of a generic
/// struct at scalars included — is held as its bytes, with no count.
#[test]
fn rc_classifier_holds_structs_and_tuples_of_scalars_as_bytes() {
    for parameter in ["p", "ints", "pair", "maybe_int"] {
        assert!(
            !counted(parameter, &[]),
            "`{parameter}` should not be counted"
        );
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

/// `void` holds no count of its own.
#[test]
fn rc_classifier_never_counts_void() {
    use miri::mir::rc::RcClassifier;
    use std::collections::HashSet;
    let classifier = RcClassifier::default();
    let open = HashSet::new();
    assert!(!classifier.is_counted(&TypeKind::Void, &open));
}

/// Every kind a test asks the two classifier queries about, written in the
/// program `COUNTED_DECLARATIONS` declares.
fn every_kind() -> Vec<TypeKind> {
    use miri::ast::types::FunctionTypeData;
    let ty = |kind: TypeKind| Type::new(kind, Span::default());
    let custom = |name: &str, args: Vec<TypeKind>| {
        TypeKind::Custom(
            name.to_string(),
            (!args.is_empty()).then(|| args.into_iter().map(expr_ty).collect()),
        )
    };
    let scalars = [
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
        TypeKind::RawPtr,
    ];
    let int = || Box::new(expr_ty(TypeKind::Int));
    let text = || Box::new(expr_ty(TypeKind::String));
    let mut kinds: Vec<TypeKind> = scalars.to_vec();
    kinds.extend([
        TypeKind::String,
        TypeKind::Void,
        TypeKind::List(int()),
        TypeKind::List(text()),
        TypeKind::Array(int(), Box::new(expr_ty(TypeKind::Int))),
        TypeKind::Map(text(), int()),
        TypeKind::Set(int()),
        TypeKind::Tuple(vec![expr_ty(TypeKind::Int), expr_ty(TypeKind::Int)]),
        TypeKind::Tuple(vec![expr_ty(TypeKind::Int), expr_ty(TypeKind::String)]),
        TypeKind::Result(int(), text()),
        TypeKind::Future(int()),
        TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: Vec::new(),
            return_type: None,
        })),
        TypeKind::Option(Box::new(ty(TypeKind::Int))),
        TypeKind::Option(Box::new(ty(TypeKind::String))),
        TypeKind::Option(Box::new(ty(custom("Node", vec![])))),
        TypeKind::Linear(Box::new(ty(TypeKind::String))),
        custom("P", vec![]),
        custom("Pair", vec![TypeKind::Int, TypeKind::Float]),
        custom("Pair", vec![TypeKind::String, TypeKind::Int]),
        custom("Node", vec![]),
        custom("V", vec![]),
        custom("Meters", vec![]),
        custom("T", vec![]),
        custom("Self", vec![]),
        custom("Vec3", vec![TypeKind::F32]),
    ]);
    kinds
}

/// What Perceus counts (`is_counted_in`) and what a drop releases out of a
/// slot (`is_slot_counted`, which every drop thunk asks of each field and
/// element) agree on every kind, with no type parameter open and with one
/// open: a disagreement is a retain no release balances, or a release of a
/// reference never taken.
#[test]
fn a_slot_is_counted_exactly_when_its_value_is() {
    use miri::mir::rc::is_counted_in;
    use std::collections::HashSet;
    let result = crate::type_checker::utils::type_checker_result(COUNTED_DECLARATIONS);
    let definitions = result.type_checker.type_definitions().clone();
    let open: HashSet<String> = HashSet::from(["T".to_string()]);
    let scopes = [
        ("closed", LayoutScope::program(&definitions)),
        ("open T", LayoutScope::new(&definitions, &open)),
    ];
    let disagreements: Vec<String> = every_kind()
        .iter()
        .flat_map(|kind| {
            scopes
                .iter()
                .map(move |(label, scope)| (kind, label, *scope))
        })
        .filter(|(kind, _, scope)| is_counted_in(kind, *scope) != is_slot_counted(kind, *scope))
        .map(|(kind, label, scope)| {
            format!(
                "{kind:?} ({label}): value counted {}, slot counted {}",
                is_counted_in(kind, scope),
                is_slot_counted(kind, scope)
            )
        })
        .collect();
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}
