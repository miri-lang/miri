// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;

#[test]
fn test_type_new() {
    let kind = TypeKind::Int;
    let span = Span { start: 10, end: 20 };
    let typ = Type::new(kind.clone(), span);

    assert_eq!(typ.kind, kind);
    assert_eq!(typ.span, span);
}
