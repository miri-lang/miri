// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Comprehensive tests for import validation, visibility enforcement, and
//! transitive-dependency leakage prevention.
//!
//! # Bug targets
//!
//! 1. **Selective import of non-existent names**: `use local.m.{Real, Ghost}` must
//!    report an error for `Ghost`, even though `Real` exists.
//! 2. **Private bypass via selective import**: `use local.m.{PrivateThing}` must
//!    be rejected despite explicitly naming the item.
//! 3. **Transitive leakage via wildcard**: importing A (which internally imports B)
//!    must not make B's types/functions directly accessible.
//! 4. **Transitive leakage via selective**: explicitly naming a type that is only
//!    transitively available must be rejected.

use super::utils::*;

// ---------------------------------------------------------------------------
// Group 1: Selective import — names that do not exist in the module
// ---------------------------------------------------------------------------

/// The known bug: `use local.m.{Real, Ghost, Whatever}` silently succeeds even
/// though only `Real` is defined. The compiler should error on `Ghost`/`Whatever`.
#[test]
fn test_selective_import_of_nonexistent_name_errors() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.models.user.{User, Printable, Ghost}\n",
                    "let u = User(name: \"x\", age: 1)\n"
                ),
            ),
            (
                "models/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n"),
            ),
        ],
        "Printable",
    );
}

/// All requested names are non-existent — must be an error.
#[test]
fn test_selective_import_all_names_nonexistent_errors() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                "use local.empty_module.{Ghost, Phantom, Shadow}\n",
            ),
            ("empty_module.mi", "// deliberately empty\n"),
        ],
        "Ghost",
    );
}

/// Exact reproduction of the known bug from the issue description:
/// `use local.users.user.{User, Printable, SomeThingElse, Whatever, WhatsAppp}`
/// while only `User` is defined in that module.
#[test]
fn test_selective_import_many_nonexistent_names_known_bug() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.users.user.{User, Printable, SomeThingElse, Whatever, WhatsAppp}\n",
                    "let u = User(name: \"a\", age: 0)\n",
                ),
            ),
            (
                "users/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n"),
            ),
        ],
        "Printable",
    );
}

/// Same as above but checks that a second non-existent name is also caught.
#[test]
fn test_selective_import_multiple_nonexistent_all_reported() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.users.user.{User, Printable, SomeThingElse, Whatever, WhatsAppp}\n",
                    "let u = User(name: \"a\", age: 0)\n",
                ),
            ),
            (
                "users/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n"),
            ),
        ],
        "SomeThingElse",
    );
}

/// Selective import of a function that does not exist must error.
#[test]
fn test_selective_import_nonexistent_function_errors() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.utils.math.{add, nonexistent_fn}\n",
                    "let r = add(1, 2)\n"
                ),
            ),
            (
                "utils/math.mi",
                "fn add(a int, b int) int:\n    return a + b\n",
            ),
        ],
        "nonexistent_fn",
    );
}

/// Selective import of a constant that does not exist must error.
#[test]
fn test_selective_import_nonexistent_constant_errors() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.config.settings.{REAL_LIMIT, FAKE_LIMIT}\n",),
            ),
            ("config/settings.mi", "const REAL_LIMIT int = 100\n"),
        ],
        "FAKE_LIMIT",
    );
}

// ---------------------------------------------------------------------------
// Group 2: Private entity bypass via selective import
// ---------------------------------------------------------------------------

/// Explicitly naming a private struct in a selective import must still be rejected.
#[test]
fn test_selective_import_cannot_bypass_private_struct() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.models.secret.{SecretData}\n",
                    "let s = SecretData(value: 1)\n"
                ),
            ),
            (
                "models/secret.mi",
                concat!("private struct SecretData\n", "    value int\n"),
            ),
        ],
        "not visible",
    );
}

/// Explicitly naming a private function in a selective import must still be rejected.
#[test]
fn test_selective_import_cannot_bypass_private_function() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.utils.crypto.{private_hash}\n",
                    "let h = private_hash()\n"
                ),
            ),
            (
                "utils/crypto.mi",
                concat!(
                    "private fn private_hash() int:\n",
                    "    return 42\n",
                    "fn public_verify() int:\n",
                    "    return 1\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// Explicitly naming a private class in a selective import must still be rejected.
#[test]
fn test_selective_import_cannot_bypass_private_class() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.core.internals.{InternalEngine}\n",
                    "let e = InternalEngine()\n"
                ),
            ),
            (
                "core/internals.mi",
                concat!(
                    "private class InternalEngine\n",
                    "    fn init()\n",
                    "        0\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// Explicitly naming a private constant in a selective import must still be rejected.
#[test]
fn test_selective_import_cannot_bypass_private_constant() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.config.secrets.{DB_PASSWORD}\n",
                    "println(f'{DB_PASSWORD}')\n",
                ),
            ),
            (
                "config/secrets.mi",
                concat!(
                    "private const DB_PASSWORD int = 1234\n",
                    "const MAX_CONN int = 10\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// Explicitly naming a private enum in a selective import must still be rejected.
#[test]
fn test_selective_import_cannot_bypass_private_enum() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.models.access.{Role}\n", "let r = Role.Admin\n",),
            ),
            (
                "models/access.mi",
                concat!("private enum Role\n", "    Admin\n", "    Guest\n"),
            ),
        ],
        "not visible",
    );
}

// ---------------------------------------------------------------------------
// Group 3: Transitive type leakage — wildcard imports
// ---------------------------------------------------------------------------

/// When module A does `use local.b` internally, a wildcard import of A by main
/// must NOT make B's types directly accessible in main.
#[test]
fn test_wildcard_import_does_not_leak_transitive_struct() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // imports A wildcard; tries to use B's InternalConfig directly
                concat!(
                    "use local.service.handler\n",
                    "let c = InternalConfig(timeout: 30)\n",
                ),
            ),
            (
                "service/handler.mi",
                // handler imports config for its own use
                concat!(
                    "use local.service.config\n",
                    "fn handle() int:\n",
                    "    return 1\n",
                ),
            ),
            (
                "service/config.mi",
                concat!("struct InternalConfig\n", "    timeout int\n"),
            ),
        ],
        "Undefined",
    );
}

/// When module A imports B for internal use, a wildcard import of A must NOT make
/// B's free functions directly callable in main.
#[test]
fn test_wildcard_import_does_not_leak_transitive_function() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // imports A wildcard; tries to call B's helper_fn directly
                concat!("use local.service.api\n", "let r = internal_helper()\n",),
            ),
            (
                "service/api.mi",
                // api imports utils for its own use
                concat!(
                    "use local.service.utils\n",
                    "fn handle() int:\n",
                    "    return internal_helper()\n",
                ),
            ),
            (
                "service/utils.mi",
                "fn internal_helper() int:\n    return 99\n",
            ),
        ],
        "Undefined",
    );
}

/// When module A imports a trait from B and uses it internally, a wildcard
/// import of A must NOT make the trait accessible in main for type annotations.
#[test]
fn test_wildcard_import_does_not_leak_transitive_trait_for_use() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // imports A wildcard; tries to use Printable trait directly
                concat!(
                    "use local.models.document\n",
                    "fn show(p Printable)\n",
                    "    1\n",
                ),
            ),
            (
                "models/document.mi",
                concat!(
                    "use local.traits.printable\n",
                    "public class Report implements Printable\n",
                    "    fn print()\n",
                    "        1\n",
                ),
            ),
            ("traits/printable.mi", "trait Printable\n    fn print()\n"),
        ],
        // The transitive trait is not in scope for the importer;
        // the error may be "Unknown type" or "Undefined" depending on the code path.
        "Printable",
    );
}

/// Enum defined in a transitively imported module must not be accessible
/// after a wildcard import of the direct module.
#[test]
fn test_wildcard_import_does_not_leak_transitive_enum() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.core.processor\n", "let s = TaskState.Pending\n",),
            ),
            (
                "core/processor.mi",
                concat!(
                    "use local.core.states\n",
                    "fn process() int:\n",
                    "    return 1\n",
                ),
            ),
            (
                "core/states.mi",
                concat!("enum TaskState\n", "    Pending\n", "    Done\n"),
            ),
        ],
        "Undefined",
    );
}

/// Constant defined in a transitively imported module must not be accessible.
#[test]
fn test_wildcard_import_does_not_leak_transitive_constant() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.core.runner\n",
                    "println(f'{INTERNAL_TIMEOUT}')\n",
                ),
            ),
            (
                "core/runner.mi",
                concat!(
                    "use local.core.timings\n",
                    "fn run() int:\n",
                    "    return INTERNAL_TIMEOUT\n",
                ),
            ),
            ("core/timings.mi", "const INTERNAL_TIMEOUT int = 5000\n"),
        ],
        "Undefined",
    );
}

// ---------------------------------------------------------------------------
// Group 4: Transitive type leakage — selective imports
// ---------------------------------------------------------------------------

/// Even explicitly naming a transitively-available type in a selective import
/// must not make it accessible: the type does not belong to the imported module.
#[test]
fn test_selective_import_cannot_select_transitive_type() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // Tries to explicitly select `InternalConfig` from handler,
                // but InternalConfig is only defined in service/config.mi.
                concat!(
                    "use local.service.handler.{InternalConfig}\n",
                    "let c = InternalConfig(timeout: 10)\n",
                ),
            ),
            (
                "service/handler.mi",
                concat!(
                    "use local.service.config\n",
                    "fn handle() int:\n",
                    "    return 1\n",
                ),
            ),
            (
                "service/config.mi",
                concat!("struct InternalConfig\n", "    timeout int\n"),
            ),
        ],
        // Should error: InternalConfig is not defined in handler, only transitively available
        "Undefined",
    );
}

/// Explicitly selecting a function that only exists transitively must error.
#[test]
fn test_selective_import_cannot_select_transitive_function() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // internal_helper is only in utils, not in api
                concat!(
                    "use local.service.api.{internal_helper}\n",
                    "let r = internal_helper()\n",
                ),
            ),
            (
                "service/api.mi",
                concat!(
                    "use local.service.utils\n",
                    "fn handle() int:\n",
                    "    return internal_helper()\n",
                ),
            ),
            (
                "service/utils.mi",
                "fn internal_helper() int:\n    return 99\n",
            ),
        ],
        "Undefined",
    );
}

// ---------------------------------------------------------------------------
// Group 5: Three-hop transitive chain — no leakage at any level
// ---------------------------------------------------------------------------

/// main → A → B → C: only A's public symbols must be accessible from main.
/// B and C symbols must not leak.
#[test]
fn test_three_hop_chain_does_not_leak_b_types() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // Try to use B's BModel directly after importing A
                concat!("use local.layers.a\n", "let m = BModel(x: 1)\n",),
            ),
            (
                "layers/a.mi",
                concat!("use local.layers.b\n", "fn a_fn() int:\n", "    return 1\n",),
            ),
            (
                "layers/b.mi",
                concat!("use local.layers.c\n", "struct BModel\n", "    x int\n",),
            ),
            ("layers/c.mi", "struct CData\n    y int\n"),
        ],
        "Undefined",
    );
}

/// main → A → B → C: C's types must not leak through A.
#[test]
fn test_three_hop_chain_does_not_leak_c_types() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.layers.a\n", "let d = CData(y: 2)\n",),
            ),
            (
                "layers/a.mi",
                concat!("use local.layers.b\n", "fn a_fn() int:\n", "    return 1\n",),
            ),
            (
                "layers/b.mi",
                concat!("use local.layers.c\n", "struct BModel\n", "    x int\n",),
            ),
            ("layers/c.mi", "struct CData\n    y int\n"),
        ],
        "Undefined",
    );
}

// ---------------------------------------------------------------------------
// Group 6: Re-import of already-loaded module — selective must still filter
// ---------------------------------------------------------------------------

/// If module M is already loaded (because another import caused it to load),
/// a second selective import `use local.m.{TypeA}` must still hide other symbols
/// from M (e.g. `TypeB`) that are not listed.
#[test]
fn test_reimport_already_loaded_selective_still_filters() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    // This triggers loading of models.mi as a side-effect
                    "use local.facade\n",
                    // Now explicitly import models with only TypeA
                    "use local.models.{TypeA}\n",
                    // TypeB must NOT be visible despite models being already loaded
                    "let b = TypeB(v: 2)\n",
                ),
            ),
            (
                "facade.mi",
                concat!(
                    "use local.models\n",
                    "fn facade_fn() int:\n",
                    "    return 1\n",
                ),
            ),
            (
                "models.mi",
                concat!(
                    "struct TypeA\n",
                    "    v int\n",
                    "struct TypeB\n",
                    "    v int\n",
                ),
            ),
        ],
        "Undefined",
    );
}

// ---------------------------------------------------------------------------
// Group 7: Module alias — does not expose transitive types
// ---------------------------------------------------------------------------

/// A module alias `use local.a as A` must not make transitive types from B
/// accessible via the alias namespace `A.BType`.
#[test]
fn test_module_alias_does_not_expose_transitive_types() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                // Try to access InternalConfig via the module alias
                concat!(
                    "use local.service.handler as H\n",
                    "let c = H.InternalConfig(timeout: 5)\n",
                ),
            ),
            (
                "service/handler.mi",
                concat!(
                    "use local.service.config\n",
                    "fn handle() int:\n",
                    "    return 1\n",
                ),
            ),
            (
                "service/config.mi",
                concat!("struct InternalConfig\n", "    timeout int\n"),
            ),
        ],
        // Either "not found" in handler's namespace, or "Undefined"
        "InternalConfig",
    );
}

// ---------------------------------------------------------------------------
// Group 8: Wildcard private visibility — extra types
// ---------------------------------------------------------------------------

/// A private trait in a module is not importable via wildcard.
#[test]
fn test_wildcard_import_does_not_expose_private_trait() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.behaviors.runner\n",
                    "fn use_it(r Runnable)\n",
                    "    r.run()\n",
                ),
            ),
            (
                "behaviors/runner.mi",
                concat!(
                    "private trait Runnable\n",
                    "    fn run()\n",
                    "fn run_all() int:\n",
                    "    return 1\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// A private enum variant accessor must not be usable from importing module.
#[test]
fn test_wildcard_import_does_not_expose_private_enum() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.status\n", "let s = InternalState.Running\n",),
            ),
            (
                "status.mi",
                concat!(
                    "private enum InternalState\n",
                    "    Running\n",
                    "    Stopped\n",
                    "const OK int = 1\n",
                ),
            ),
        ],
        "not visible",
    );
}

// ---------------------------------------------------------------------------
// Group 9: Struct field visibility across module boundary
// ---------------------------------------------------------------------------

/// A private field on an imported class must not be directly accessible
/// from the importing module.
#[test]
fn test_cross_module_private_field_inaccessible() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.account\n",
                    "let a = Account()\n",
                    "println(f'{a._balance}')\n",
                ),
            ),
            (
                "models/account.mi",
                concat!(
                    "use system.io\n",
                    "public class Account\n",
                    "    private var _balance int\n",
                    "\n",
                    "    fn init()\n",
                    "        self._balance = 0\n",
                    "\n",
                    "    fn deposit(amount int)\n",
                    "        self._balance = self._balance + amount\n",
                ),
            ),
        ],
        "Private",
    );
}

// ---------------------------------------------------------------------------
// Group 10: Selective import of a type that is defined in another transitive module
//           but has the same name as something in the direct module
// ---------------------------------------------------------------------------

/// When a module A defines TypeA and transitively imports TypeB from module B,
/// a selective import `use local.a.{TypeA}` must expose TypeA (from A) but NOT
/// TypeB (from B, which is transitive).
#[test]
fn test_selective_import_exposes_direct_type_but_not_sibling_transitive() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.top.module.{TypeA}\n",
                    // TypeB is only in base, not in module — must be invisible
                    "let b = TypeB(y: 5)\n",
                ),
            ),
            (
                "top/module.mi",
                concat!("use local.top.base\n", "struct TypeA\n", "    x int\n",),
            ),
            ("top/base.mi", "struct TypeB\n    y int\n"),
        ],
        "Undefined",
    );
}

// ---------------------------------------------------------------------------
// Group 11: Verify that using a non-imported sibling entity is always an error
// ---------------------------------------------------------------------------

/// A type defined in the same module file but NOT listed in a selective import
/// must remain invisible — even for functions, not just types.
#[test]
fn test_selective_import_non_listed_function_is_invisible() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.utils.math.{add}\n",
                    // `multiply` exists in math but was not imported
                    "let r = multiply(3, 4)\n",
                ),
            ),
            (
                "utils/math.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "fn multiply(a int, b int) int:\n",
                    "    return a * b\n",
                ),
            ),
        ],
        "Undefined",
    );
}

/// A constant in the same module file but NOT listed in a selective import
/// must remain invisible.
#[test]
fn test_selective_import_non_listed_constant_is_invisible() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.config.limits.{MAX_RETRIES}\n",
                    // TIMEOUT exists in the module but was not imported
                    "println(f'{TIMEOUT}')\n",
                ),
            ),
            (
                "config/limits.mi",
                concat!("const MAX_RETRIES int = 5\n", "const TIMEOUT int = 30\n",),
            ),
        ],
        "Undefined",
    );
}

// ---------------------------------------------------------------------------
// Group 12: Positive control — selective import with valid names still works
// ---------------------------------------------------------------------------

/// Sanity check: a selective import that lists only real, public names still works.
#[test]
fn test_selective_import_with_all_valid_names_succeeds() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.math.{add, multiply}\n",
                    "let r = add(multiply(2, 3), 4)\n",
                    "println(f'{r}')\n",
                ),
            ),
            (
                "utils/math.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "fn multiply(a int, b int) int:\n",
                    "    return a * b\n",
                ),
            ),
        ],
        "10",
    );
}

/// Wildcard import of a module exposes only public things from that exact module.
#[test]
fn test_wildcard_import_exposes_public_symbols_only() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.math\n",
                    "let r = add(3, 4)\n",
                    "println(f'{r}')\n",
                ),
            ),
            (
                "utils/math.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "private fn secret() int:\n",
                    "    return 0\n",
                ),
            ),
        ],
        "7",
    );
}
