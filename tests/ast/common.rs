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

/// The runtimes the Makefile's `FRAME_POINTER_RUNTIMES` builds a
/// frame-pointer copy of, by name.
fn makefile_frame_pointer_runtimes() -> Vec<String> {
    let makefile = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Makefile"))
        .expect("the Makefile should be readable");
    let line = makefile
        .lines()
        .find_map(|line| line.strip_prefix("FRAME_POINTER_RUNTIMES :="))
        .expect("the Makefile should define FRAME_POINTER_RUNTIMES");
    line.split_whitespace()
        .map(|path| {
            path.strip_prefix("src/runtime/")
                .expect("each entry should be a runtime directory")
                .to_string()
        })
        .collect()
}

/// Every runtime directory, so a runtime added later is checked too.
fn runtime_names() -> Vec<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/runtime");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("src/runtime should be readable")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `make runtimes` builds a frame-pointer copy of exactly the runtimes a
/// profile build links one of, and every runtime gets one: a profiler
/// walking frame pointers breaks a stack at the first runtime without them.
#[test]
fn every_runtime_has_the_frame_pointer_copy_the_makefile_builds() {
    let built = makefile_frame_pointer_runtimes();
    for name in runtime_names() {
        let kind = RuntimeKind::from_name(&name)
            .unwrap_or_else(|| panic!("src/runtime/{name} names no runtime kind"));
        assert!(
            kind.has_frame_pointer_build(),
            "the `{name}` runtime has no frame-pointer build"
        );
        assert!(
            built.contains(&name),
            "the Makefile builds no frame-pointer copy of `{name}`: {built:?}"
        );
    }
}
