// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

/// The entry-point file's directory is the project root.
/// A module at `utils/math.mi` imported via `use local.utils.math` must be
/// found relative to that root, not the working directory.
#[test]
fn test_local_module_resolves_relative_to_entry_point() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.math\n",
                    "println(f'{square(6)}')\n",
                ),
            ),
            ("utils/math.mi", "fn square(n int) int:\n    return n * n\n"),
        ],
        "36",
    );
}

/// `local.*` imports made by a deeply-nested module must still resolve
/// relative to the entry-point's directory (the project root), not relative
/// to the importing module's own directory.
///
/// Dependency chain:
///   app.mi  →  utils/math/calculations.mi  →  local.my_module.some
///
/// `my_module/some.mi` lives at the project root, NOT inside `utils/math/`.
#[test]
fn test_project_root_is_always_entry_point_directory() {
    assert_project_runs_with_output(
        &[
            (
                "app.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.math.calculations\n",
                    "println(f'{double(7)}')\n",
                ),
            ),
            (
                "utils/math/calculations.mi",
                concat!(
                    "use local.my_module.some\n",
                    "fn double(n int) int:\n",
                    "    return multiply(n, 2)\n",
                ),
            ),
            (
                "my_module/some.mi",
                "fn multiply(a int, b int) int:\n    return a * b\n",
            ),
        ],
        "14",
    );
}
