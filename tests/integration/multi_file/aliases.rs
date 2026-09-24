// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ---------------------------------------------------------------------------
// Module-level alias: `use X as M`
// ---------------------------------------------------------------------------

/// `use local.utils.calc as C` makes functions callable as `C.add(3, 4)`.
/// This is the core acceptance criterion for module aliasing.
#[test]
fn test_module_alias_basic_call() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.calc as C\n",
                    "let result = C.add(3, 4)\n",
                    "println(f'{result}')\n",
                ),
            ),
            (
                "utils/calc.mi",
                concat!("fn add(a int, b int) int:\n", "    return a + b\n",),
            ),
        ],
        "7",
    );
}

/// Module alias with multiple function calls.
#[test]
fn test_module_alias_multiple_calls() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.calc as C\n",
                    "let a = C.add(10, 5)\n",
                    "let b = C.mul(3, 4)\n",
                    "println(f'{a}')\n",
                    "println(f'{b}')\n",
                ),
            ),
            (
                "utils/calc.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "fn mul(a int, b int) int:\n",
                    "    return a * b\n",
                ),
            ),
        ],
        "15\n12",
    );
}

/// Two modules imported under different aliases work independently.
#[test]
fn test_two_module_aliases() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.calc as C\n",
                    "use local.utils.strings as S\n",
                    "let n = C.add(1, 2)\n",
                    "let s = S.repeat(\"hi\", 2)\n",
                    "println(f'{n}')\n",
                    "println(s)\n",
                ),
            ),
            (
                "utils/calc.mi",
                concat!("fn add(a int, b int) int:\n", "    return a + b\n",),
            ),
            (
                "utils/strings.mi",
                concat!(
                    "fn repeat(s String, n int) String:\n",
                    "    var result = \"\"\n",
                    "    var i = 0\n",
                    "    while i < n:\n",
                    "        result = f'{result}{s}'\n",
                    "        i = i + 1\n",
                    "    return result\n",
                ),
            ),
        ],
        "3\nhihi",
    );
}

/// Module alias with a short single-letter name (common convention like `M`).
#[test]
fn test_module_alias_single_letter() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.math as M\n",
                    "let r = M.square(5)\n",
                    "println(f'{r}')\n",
                ),
            ),
            (
                "math.mi",
                concat!("fn square(x int) int:\n", "    return x * x\n",),
            ),
        ],
        "25",
    );
}

// ---------------------------------------------------------------------------
// Item alias: `use X.{foo as bar}`
// ---------------------------------------------------------------------------

/// `use local.utils.calc.{add as plus}` makes `add` callable as `plus`.
#[test]
fn test_item_alias_basic() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.calc.{add as plus}\n",
                    "let result = plus(10, 20)\n",
                    "println(f'{result}')\n",
                ),
            ),
            (
                "utils/calc.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "fn mul(a int, b int) int:\n",
                    "    return a * b\n",
                ),
            ),
        ],
        "30",
    );
}

/// Mixed selective import: one item aliased, one not.
#[test]
fn test_item_alias_mixed_selective() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.calc.{add as plus, mul}\n",
                    "let a = plus(3, 4)\n",
                    "let b = mul(2, 5)\n",
                    "println(f'{a}')\n",
                    "println(f'{b}')\n",
                ),
            ),
            (
                "utils/calc.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "fn mul(a int, b int) int:\n",
                    "    return a * b\n",
                ),
            ),
        ],
        "7\n10",
    );
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

/// A private function accessed via module alias must also be rejected.
#[test]
fn test_module_alias_private_function_inaccessible() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!("use local.utils.calc as C\n", "let result = C.secret()\n",),
            ),
            (
                "utils/calc.mi",
                concat!(
                    "fn add(a int, b int) int:\n",
                    "    return a + b\n",
                    "private fn secret() int:\n",
                    "    return 42\n",
                ),
            ),
        ],
        "not visible",
    );
}

/// Calling an undefined member on a module alias reports an error.
#[test]
fn test_module_alias_undefined_member_error() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.utils.calc as C\n",
                    "let result = C.nonexistent(1, 2)\n",
                ),
            ),
            (
                "utils/calc.mi",
                concat!("fn add(a int, b int) int:\n", "    return a + b\n",),
            ),
        ],
        "nonexistent",
    );
}

/// A module's own constants named like mathematical ones read their declared
/// values through the alias; the name alone picks no value.
#[test]
fn test_module_alias_reads_constants_named_like_math_constants() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.consts as K\n",
                    "println(f\"{K.E}\")\n",
                    "println(f\"{K.PI}\")\n",
                    "println(f\"{K.INF}\")\n",
                ),
            ),
            (
                "utils/consts.mi",
                concat!(
                    "let E = 5\n",
                    "let PI float = 1.5\n",
                    "let INF = \"none\"\n",
                ),
            ),
        ],
        "5\n1.5\nnone",
    );
}

/// Alias member reads of a module's boolean and string constants carry their
/// declared values.
#[test]
fn test_module_alias_reads_bool_and_string_constants() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.flags as F\n",
                    "println(f\"{F.VERBOSE}\")\n",
                    "println(F.LABEL)\n",
                ),
            ),
            (
                "utils/flags.mi",
                concat!("let VERBOSE = true\n", "const LABEL = \"tag\"\n",),
            ),
        ],
        "true\ntag",
    );
}

/// Reading a module-level binding with no compile-time value through an alias
/// is refused, as it is through a plain import.
#[test]
fn test_module_alias_binding_without_constant_initializer_is_refused() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.consts as K\n",
                    "println(f\"{K.COUNT}\")\n",
                ),
            ),
            (
                "utils/consts.mi",
                concat!("fn make() int\n", "    return 3\n", "let COUNT = make()\n",),
            ),
        ],
        "'COUNT' has no value at run time",
    );
}
