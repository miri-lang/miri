// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test};

/// Selective import `use system.io.{println}` should make only `println` available.
#[test]
fn selective_import_makes_only_named_items_available() {
    type_checker_test(
        r#"
use system.io.{println}

println("hello")
"#,
    );
}

/// A selective import must still hide the module's other symbols. `system.io`
/// can no longer demonstrate this (the implicit prelude re-exports all of it),
/// so use a non-prelude module: importing only `sqrt` must leave `abs` undefined.
#[test]
fn selective_import_rejects_non_imported_items() {
    type_checker_error_test(
        r#"
use system.math.{sqrt}

let x = abs(-1.0)
"#,
        "Undefined variable: abs",
    );
}

/// Importing multiple items should make all of them available.
#[test]
fn selective_import_multiple_items() {
    type_checker_test(
        r#"
use system.io.{println, print}

println("hello")
print("world")
"#,
    );
}

/// Wildcard import should still import everything.
#[test]
fn wildcard_import_still_imports_everything() {
    type_checker_test(
        r#"
use system.io.*

println("hello")
print("world")
eprint("err")
eprintln("errln")
"#,
    );
}

/// Simple (non-selective) import should still import everything.
#[test]
fn simple_import_still_imports_everything() {
    type_checker_test(
        r#"
use system.io

println("hello")
print("world")
eprint("err")
eprintln("errln")
"#,
    );
}
