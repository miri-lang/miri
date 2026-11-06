// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::type_arguments::type_argument;
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::structural_elements::structural_element_types;
use miri::error::syntax::Span;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

/// A pair whose second element is `kind`.
fn pair_with(kind: TypeKind) -> TypeKind {
    TypeKind::Tuple(vec![
        type_argument(ty(TypeKind::Int)),
        type_argument(ty(kind)),
    ])
}

/// A body holding one `List<(int, Pair<U>)>` local, leaving `open` open and
/// having bound `bound`.
fn body_holding_pairs_of_u(open: &[&str], bound: &[&str]) -> miri::mir::Body {
    let pair_of_u = TypeKind::Custom(
        "Pair".to_string(),
        Some(vec![type_argument(ty(TypeKind::Custom(
            "U".to_string(),
            None,
        )))]),
    );
    let list = TypeKind::Custom(
        "List".to_string(),
        Some(vec![type_argument(ty(pair_with(pair_of_u)))]),
    );
    let mut body = miri::mir::Body::new(0, Span::default(), miri::mir::ExecutionModel::Cpu);
    body.new_local(miri::mir::LocalDecl::new(ty(list), Span::default()));
    body.generics.open_params = open.iter().map(|name| name.to_string()).collect();
    body.generics.bound_params = bound.iter().map(|name| name.to_string()).collect();
    body
}

#[test]
fn a_parameter_bound_by_any_holder_of_an_entry_type_stays_bound() {
    // One holder leaves `U` open; the other bound it and still spells it, a
    // substitution it missed. The thunk has to see the miss, not the open `U`.
    let shared = body_holding_pairs_of_u(&["U"], &[]);
    let instantiated = body_holding_pairs_of_u(&[], &["U"]);
    let elements =
        structural_element_types(&[("shared", &shared), ("instantiated", &instantiated)]);

    let element = elements
        .iter()
        .find(|element| matches!(element.kind, TypeKind::Tuple(_)))
        .expect("the pair entry type is collected");
    assert!(element.open_params.contains("U"));
    assert!(element.bound_params.contains("U"));
}
