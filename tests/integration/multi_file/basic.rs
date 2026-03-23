// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_local_module_function_call() {
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
                "fn add(a int, b int) int:\n    return a + b\n",
            ),
        ],
        "7",
    );
}

#[test]
fn test_local_multiple_modules() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.math\n",
                    "use local.utils.strings\n",
                    "let n = add(10, 5)\n",
                    "let s = greet(\"world\")\n",
                    "println(f'{n}')\n",
                    "println(s)\n",
                ),
            ),
            (
                "utils/math.mi",
                "fn add(a int, b int) int:\n    return a + b\n",
            ),
            (
                "utils/strings.mi",
                concat!(
                    "fn greet(name String) String:\n",
                    "    return f'hello {name}'\n",
                ),
            ),
        ],
        "15",
    );
}
