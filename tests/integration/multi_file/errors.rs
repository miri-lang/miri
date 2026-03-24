// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_local_module_not_found() {
    assert_project_compiler_error(
        &[("main.mi", "use local.missing.helper\nlet x = 1\n")],
        "Module 'local.missing.helper' not found",
    );
}

// ---------------------------------------------------------------------------
// Cross-module visibility enforcement
// ---------------------------------------------------------------------------

/// A private function in a module must not be callable from an importing module.
#[test]
fn test_private_function_inaccessible_from_importer() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.utils.math\n", "let result = secret()\n",),
            ),
            (
                "utils/math.mi",
                concat!(
                    "private fn secret() int:\n",
                    "    return 42\n",
                    "fn public_add(a int, b int) int:\n",
                    "    return a + b\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// A public function in a module is callable from an importing module.
#[test]
fn test_public_function_accessible_from_importer() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.math\n",
                    "let result = add(3, 4)\n",
                    "println(f'{result}')\n",
                ),
            ),
            (
                "utils/math.mi",
                concat!(
                    "public fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "private fn helper() int:\n",
                    "    return 0\n",
                ),
            ),
        ],
        "7",
    );
}

/// A private struct in a module must not be constructible from an importing module.
#[test]
fn test_private_struct_inaccessible_from_importer() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.models.internal\n", "let s = Secret(value: 1)\n",),
            ),
            (
                "models/internal.mi",
                concat!("private struct Secret\n", "    value int\n",),
            ),
        ],
        "not visible",
    );
}

/// A private constant in a module must not be readable from an importing module.
#[test]
fn test_private_constant_inaccessible_from_importer() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.config.limits\n",
                    "println(f'{SECRET_KEY}')\n",
                ),
            ),
            (
                "config/limits.mi",
                concat!(
                    "private const SECRET_KEY int = 99\n",
                    "const PUBLIC_MAX int = 10\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// A public constant in a module is readable from an importing module.
#[test]
fn test_public_constant_accessible_from_importer() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.config.limits\n",
                    "println(f'{PUBLIC_MAX}')\n",
                ),
            ),
            (
                "config/limits.mi",
                concat!(
                    "private const SECRET_KEY int = 99\n",
                    "const PUBLIC_MAX int = 10\n",
                ),
            ),
        ],
        "10",
    );
}

// ---------------------------------------------------------------------------
// Circular dependency detection
// ---------------------------------------------------------------------------

/// a.mi imports b.mi which imports a.mi — must be an error, not a hang/silent skip.
#[test]
fn test_circular_import_two_modules() {
    assert_project_compiler_error(
        &[
            ("main.mi", "use local.a\n"),
            ("a.mi", "use local.b\n"),
            ("b.mi", "use local.a\n"),
        ],
        "Circular import",
    );
}

/// Self-import: a module that directly imports itself.
#[test]
fn test_circular_import_self() {
    assert_project_compiler_error(
        &[
            ("main.mi", "use local.self_import\n"),
            ("self_import.mi", "use local.self_import\n"),
        ],
        "Circular import",
    );
}

/// Three-module chain: main → a → b → c → a.
#[test]
fn test_circular_import_three_module_chain() {
    assert_project_compiler_error(
        &[
            ("main.mi", "use local.chain_a\n"),
            ("chain_a.mi", "use local.chain_b\n"),
            ("chain_b.mi", "use local.chain_c\n"),
            ("chain_c.mi", "use local.chain_a\n"),
        ],
        "Circular import",
    );
}

/// A selective local import must not expose types that were not listed.
#[test]
fn test_local_selective_import_rejects_non_imported_type() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.models.user.{User}\n", "let x = Role.Admin\n",),
            ),
            (
                "models/user.mi",
                concat!(
                    "struct User\n",
                    "    name String\n",
                    "enum Role\n",
                    "    Admin\n",
                    "    Guest\n",
                ),
            ),
        ],
        "Undefined",
    );
}
