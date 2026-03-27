// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Safety-net tests for the hardcoded method names in
//! `src/mir/lowering/dispatch.rs`.
//!
//! `dispatch.rs` contains five method-name string literals that are matched
//! before normal class-method dispatch:
//!
//! | Method | Class(es) | Reason intercept exists |
//! |--------|-----------|-------------------------|
//! | `element_at` | List, Array | Perceus needs concrete element type at call site |
//! | `get`         | List, Array | Same as element_at |
//! | `push`        | List        | Avoids generic monomorphization conflict |
//! | `insert`      | List        | Same as push |
//! | `set`         | List, Array | Monomorphization + OOB/RC correctness |
//!
//! If any of these methods is renamed in the corresponding `.mi` file the
//! intercept silently stops firing — the method call falls through to normal
//! dispatch and is likely to miscompile or crash at runtime.
//!
//! These tests read the stdlib `.mi` files and assert that every intercepted
//! method name still exists as a public method in the relevant class.  A
//! failing test is a prompt to either update the method name in `dispatch.rs`
//! or (better) resolve the underlying compiler limitation so the intercept can
//! be removed.

use std::path::PathBuf;

// ── helpers ──────────────────────────────────────────────────────────────────

fn stdlib_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib")
}

/// Read a stdlib `.mi` file relative to the stdlib root.
/// `relative` uses forward slashes: e.g. `"system/collections/list.mi"`.
fn read_mi(relative: &str) -> String {
    let mut path = stdlib_dir();
    for segment in relative.split('/') {
        path = path.join(segment);
    }
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read stdlib file {relative}: {e}"))
}

/// Return true if the `.mi` source defines a `public fn <name>(` in its body.
/// This is intentionally simple: it matches line-by-line to avoid parsing.
fn has_public_method(source: &str, name: &str) -> bool {
    let needle = format!("public fn {}(", name);
    source.lines().any(|l| l.trim().contains(&needle))
}

// ── tests ────────────────────────────────────────────────────────────────────

/// `element_at` must exist on both List and Array — the dispatch intercept
/// emits a direct index read instead of calling through to the compiled method,
/// so Perceus can see the concrete element type for RC insertion.
#[test]
fn element_at_exists_on_list_and_array() {
    let list_src = read_mi("system/collections/list.mi");
    let array_src = read_mi("system/collections/array.mi");

    assert!(
        has_public_method(&list_src, "element_at"),
        "DISPATCH INTERCEPT BROKEN: `element_at` no longer exists as a public \
         method on List in list.mi.\n\
         Update the method name at dispatch.rs (search for \"element_at\") \
         or remove the intercept if the underlying compiler limitation is resolved."
    );
    assert!(
        has_public_method(&array_src, "element_at"),
        "DISPATCH INTERCEPT BROKEN: `element_at` no longer exists as a public \
         method on Array in array.mi.\n\
         Update the method name at dispatch.rs (search for \"element_at\") \
         or remove the intercept if the underlying compiler limitation is resolved."
    );
}

/// `get` must exist on List — the intercept emits a direct index read, same
/// reason as `element_at`.
#[test]
fn get_exists_on_list() {
    let list_src = read_mi("system/collections/list.mi");

    assert!(
        has_public_method(&list_src, "get"),
        "DISPATCH INTERCEPT BROKEN: `get` no longer exists as a public \
         method on List in list.mi.\n\
         Update the method name at dispatch.rs (search for \"get\") \
         or remove the intercept if the underlying compiler limitation is resolved."
    );
}

/// `push` must exist on List — the intercept emits `miri_rt_list_push` directly
/// to avoid monomorphization conflicts across `List<T>` instantiations.
#[test]
fn push_exists_on_list() {
    let list_src = read_mi("system/collections/list.mi");

    assert!(
        has_public_method(&list_src, "push"),
        "DISPATCH INTERCEPT BROKEN: `push` no longer exists as a public \
         method on List in list.mi.\n\
         Update the method name at dispatch.rs (search for \"push\") \
         or remove the intercept once generic monomorphization is fixed."
    );
}

/// `insert` must exist on List — same monomorphization reason as `push`.
#[test]
fn insert_exists_on_list() {
    let list_src = read_mi("system/collections/list.mi");

    assert!(
        has_public_method(&list_src, "insert"),
        "DISPATCH INTERCEPT BROKEN: `insert` no longer exists as a public \
         method on List in list.mi.\n\
         Update the method name at dispatch.rs (search for \"insert\") \
         or remove the intercept once generic monomorphization is fixed."
    );
}

/// `set` must exist on both List and Array — the intercept emits a direct
/// indexed assignment to avoid monomorphization conflicts and to let codegen
/// handle OOB checking and element RC correctly.
#[test]
fn set_exists_on_list_and_array() {
    let list_src = read_mi("system/collections/list.mi");
    let array_src = read_mi("system/collections/array.mi");

    assert!(
        has_public_method(&list_src, "set"),
        "DISPATCH INTERCEPT BROKEN: `set` no longer exists as a public \
         method on List in list.mi.\n\
         Update the method name at dispatch.rs (search for \"set\") \
         or remove the intercept once generic monomorphization is fixed."
    );
    assert!(
        has_public_method(&array_src, "set"),
        "DISPATCH INTERCEPT BROKEN: `set` no longer exists as a public \
         method on Array in array.mi.\n\
         Update the method name at dispatch.rs (search for \"set\") \
         or remove the intercept once generic monomorphization is fixed."
    );
}
