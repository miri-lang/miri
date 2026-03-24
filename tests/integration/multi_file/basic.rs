// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ---------------------------------------------------------------------------
// Struct types imported from local modules
// ---------------------------------------------------------------------------

/// `use local.models.user` resolves to `models/user.mi` and imports the
/// struct type defined there (acceptance-criteria path).
#[test]
fn test_local_module_imports_struct_type() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.user\n",
                    "let u = User(name: \"Alice\", age: 30)\n",
                    "println(u.name)\n",
                ),
            ),
            (
                "models/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n",),
            ),
        ],
        "Alice",
    );
}

/// A struct defined in a local module can be passed to and returned from
/// functions in the same project.
#[test]
fn test_local_module_struct_used_in_function() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.user\n",
                    "fn describe(u User) String:\n",
                    "    return f'{u.name} is {u.age}'\n",
                    "let u = User(name: \"Bob\", age: 25)\n",
                    "println(describe(u))\n",
                ),
            ),
            (
                "models/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n"),
            ),
        ],
        "Bob is 25",
    );
}

// ---------------------------------------------------------------------------
// Enum types imported from local modules
// ---------------------------------------------------------------------------

/// Enum variants defined in a local module are accessible after import.
#[test]
fn test_local_module_imports_enum_type() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.status\n",
                    "let s = Status.Active\n",
                    "match s\n",
                    "    Status.Active: println(\"active\")\n",
                    "    Status.Inactive: println(\"inactive\")\n",
                ),
            ),
            (
                "models/status.mi",
                concat!("enum Status\n", "    Active\n", "    Inactive\n"),
            ),
        ],
        "active",
    );
}

// ---------------------------------------------------------------------------
// Constants imported from local modules
// ---------------------------------------------------------------------------

/// Constants defined in a local module are available after import.
#[test]
fn test_local_module_imports_constant() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.config.limits\n",
                    "println(f'{MAX_RETRIES}')\n",
                ),
            ),
            ("config/limits.mi", "const MAX_RETRIES int = 5\n"),
        ],
        "5",
    );
}

// ---------------------------------------------------------------------------
// Wildcard and selective imports for local modules
// ---------------------------------------------------------------------------

/// `use local.models.*` (wildcard) imports all entities from the module.
#[test]
fn test_local_module_wildcard_import() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.user.*\n",
                    "let u = User(name: \"Charlie\", age: 20)\n",
                    "println(u.name)\n",
                ),
            ),
            (
                "models/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n"),
            ),
        ],
        "Charlie",
    );
}

/// `use local.models.user.{User}` (selective) imports only the named type.
#[test]
fn test_local_module_selective_import() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.user.{User}\n",
                    "let u = User(name: \"Dana\", age: 22)\n",
                    "println(u.name)\n",
                ),
            ),
            (
                "models/user.mi",
                concat!("struct User\n", "    name String\n", "    age int\n"),
            ),
        ],
        "Dana",
    );
}

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
