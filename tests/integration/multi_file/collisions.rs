// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ---------------------------------------------------------------------------
// Wildcard import collisions
// ---------------------------------------------------------------------------

/// Two wildcard imports that both export the same function name must produce
/// a clear error pointing at the second import.
#[test]
fn test_two_wildcard_imports_same_function_name() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.math.add\n",
                    "use local.math.sub\n",
                    "let x = add(1, 2)\n",
                ),
            ),
            (
                "math/add.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "fn greet() int:\n",
                    "    return 1\n",
                ),
            ),
            (
                "math/sub.mi",
                concat!(
                    "fn sub(a int, b int) int:\n",
                    "    return a - b\n",
                    "fn greet() int:\n",
                    "    return 2\n",
                ),
            ),
        ],
        "conflicts",
    );
}

/// Two wildcard imports that both export the same constant name must produce
/// a clear error.
#[test]
fn test_two_wildcard_imports_same_constant_name() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.config.a\n", "use local.config.b\n",),
            ),
            ("config/a.mi", "const MAX int = 10\n"),
            ("config/b.mi", "const MAX int = 20\n"),
        ],
        "conflicts",
    );
}

// ---------------------------------------------------------------------------
// Selective import collisions
// ---------------------------------------------------------------------------

/// Two selective imports that both name the same identifier must produce
/// a clear error.
#[test]
fn test_two_selective_imports_same_name() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.handlers.a.{process}\n",
                    "use local.handlers.b.{process}\n",
                ),
            ),
            (
                "handlers/a.mi",
                concat!(
                    "fn process() int:\n",
                    "    return 1\n",
                    "fn other() int:\n",
                    "    return 0\n",
                ),
            ),
            (
                "handlers/b.mi",
                concat!(
                    "fn process() int:\n",
                    "    return 2\n",
                    "fn another() int:\n",
                    "    return 0\n",
                ),
            ),
        ],
        "conflicts",
    );
}

// ---------------------------------------------------------------------------
// Error message quality
// ---------------------------------------------------------------------------

/// The error message must mention the name that conflicts.
#[test]
fn test_collision_error_mentions_name() {
    assert_project_compiler_error(
        &[
            ("main.mi", concat!("use local.alpha\n", "use local.beta\n",)),
            ("alpha.mi", "fn helper() int:\n    return 1\n"),
            ("beta.mi", "fn helper() int:\n    return 2\n"),
        ],
        "helper",
    );
}

/// The error message must mention the module the name was originally imported
/// from so the user knows where the conflict comes from.
#[test]
fn test_collision_error_mentions_original_module() {
    assert_project_compiler_error(
        &[
            ("main.mi", concat!("use local.alpha\n", "use local.beta\n",)),
            ("alpha.mi", "fn helper() int:\n    return 1\n"),
            ("beta.mi", "fn helper() int:\n    return 2\n"),
        ],
        "local.alpha",
    );
}

// ---------------------------------------------------------------------------
// Regression: non-conflicting multi-module imports still work
// ---------------------------------------------------------------------------

/// Two imports that export distinct names must compile and run without error.
#[test]
fn test_two_imports_no_collision() {
    assert_project_runs(&[
        (
            "main.mi",
            concat!(
                "use system.io\n",
                "use local.utils.greet\n",
                "use local.utils.farewell\n",
                "println(greet())\n",
                "println(farewell())\n",
            ),
        ),
        (
            "utils/greet.mi",
            "fn greet() String:\n    return \"hello\"\n",
        ),
        (
            "utils/farewell.mi",
            "fn farewell() String:\n    return \"bye\"\n",
        ),
    ]);
}
