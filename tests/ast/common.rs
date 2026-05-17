// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::common::RuntimeKind;

#[test]
fn runtime_kind_from_name_known() {
    assert_eq!(RuntimeKind::from_name("core"), Some(RuntimeKind::Core));
}

#[test]
fn runtime_kind_from_name_unknown() {
    assert_eq!(RuntimeKind::from_name(""), None);
    assert_eq!(RuntimeKind::from_name("Core"), None);
    assert_eq!(RuntimeKind::from_name("std"), None);
}

#[test]
fn runtime_kind_name_roundtrip() {
    let k = RuntimeKind::Core;
    assert_eq!(RuntimeKind::from_name(k.name()), Some(k));
}

#[test]
fn runtime_kind_library_name_matches_static_lib() {
    assert_eq!(RuntimeKind::Core.library_name(), "miri_runtime_core");
}
