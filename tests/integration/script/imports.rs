// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_selective_import_runs() {
    let code = r#"
println("selective")
"#;
    assert_runs_with_output(code, "selective");
}

#[test]
fn test_selective_import_rejects_non_imported() {
    // With implicit imports, system.io functions are available globally.
    // This test verifies that selective imports from other modules still
    // properly reject non-selected symbols.
    let code = r#"
let x = is_gpu_available()
"#;
    assert_compiler_error(code, "Undefined variable");
}

/// A name a module does not declare is refused at the import whether or not
/// something loaded the module first: the prelude loads `system.io`, and an
/// earlier import loads a collection.
#[test]
fn test_a_missing_name_from_a_prelude_module_is_refused_at_the_import() {
    assert_compiler_error(
        "use system.io.{print_line}\n\nfn main()\n    println(\"x\")\n",
        "Name 'print_line' not found in module 'system.io'",
    );
}

#[test]
fn test_a_missing_name_from_an_already_loaded_module_is_refused() {
    assert_compiler_error(
        "use system.collections.list\nuse system.collections.list.{Lisst}\n\nfn main()\n    let xs = List([1])\n    println(f\"{xs.length()}\")\n",
        "Name 'Lisst' not found in module 'system.collections.list'",
    );
}

#[test]
fn test_a_missing_name_from_a_module_loaded_here_is_refused() {
    assert_compiler_error(
        "use system.math.{square_root}\n\nfn main()\n    println(\"x\")\n",
        "MER_IMP_003",
    );
}

/// A name one edit from one the module declares is suggested.
#[test]
fn test_a_misspelled_selection_names_the_close_name() {
    assert_compiler_error(
        "use system.collections.list.{Lisst}\n\nfn main()\n    println(\"x\")\n",
        "Did you mean 'List'?",
    );
}
