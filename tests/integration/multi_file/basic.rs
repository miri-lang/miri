// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ---------------------------------------------------------------------------
// Selective imports must preserve transitive trait dependencies
// ---------------------------------------------------------------------------

/// A selective import of a class that implements a trait from another module
/// must preserve the trait in type_definitions so vtables are generated.
#[test]
fn test_selective_import_preserves_transitive_trait_for_vtable() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.animal.{Dog}\n",
                    "let d = Dog(\"Rex\")\n",
                    "d.speak()\n",
                ),
            ),
            (
                "models/animal.mi",
                concat!(
                    "use system.io\n",
                    "use local.models.speaker\n",
                    "\n",
                    "public class Dog implements Speaker\n",
                    "    private let _name String\n",
                    "\n",
                    "    fn init(name String)\n",
                    "        self._name = name\n",
                    "\n",
                    "    fn speak()\n",
                    "        println(f\"{self._name} says woof\")\n",
                ),
            ),
            (
                "models/speaker.mi",
                "trait Speaker\n    fn speak()\n",
            ),
        ],
        "Rex says woof",
    );
}

/// Two traits imported transitively via a class — both must survive
/// the selective import filter.
#[test]
fn test_selective_import_preserves_multiple_transitive_traits() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.things.widget.{Widget}\n",
                    "let w = Widget(\"btn\")\n",
                    "println(w.label())\n",
                ),
            ),
            (
                "things/widget.mi",
                concat!(
                    "use local.things.named\n",
                    "use local.things.visible\n",
                    "\n",
                    "public class Widget implements Named, Visible\n",
                    "    private let _label String\n",
                    "\n",
                    "    fn init(label String)\n",
                    "        self._label = label\n",
                    "\n",
                    "    fn label() String: self._label\n",
                    "\n",
                    "    fn show() bool: true\n",
                ),
            ),
            ("things/named.mi", "trait Named\n    fn label() String\n"),
            ("things/visible.mi", "trait Visible\n    fn show() bool\n"),
        ],
        "btn",
    );
}

/// Selective import of a class must NOT leak sibling types from the
/// same module (regression guard — this test existed before but only
/// for enums; we additionally cover structs).
#[test]
fn test_selective_import_still_hides_sibling_struct() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.models.car.{Car}\n",
                    "let e = Engine(100)\n",
                ),
            ),
            (
                "models/car.mi",
                concat!(
                    "struct Engine\n",
                    "    hp int\n",
                    "\n",
                    "struct Car\n",
                    "    name String\n",
                ),
            ),
        ],
        "Undefined",
    );
}

/// Module without trailing newline: trait file ends abruptly.
#[test]
fn test_module_without_trailing_newline() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.core.greeter.{Greeter}\n",
                    "let g = Greeter()\n",
                    "g.hello()\n",
                ),
            ),
            (
                "core/greeter.mi",
                concat!(
                    "use system.io\n",
                    "use local.core.base\n",
                    "\n",
                    "public class Greeter implements Greetable\n",
                    "    fn hello()\n",
                    "        println(\"hi\")\n",
                ),
            ),
            // No trailing newline!
            ("core/base.mi", "trait Greetable\n    fn hello()"),
        ],
        "hi",
    );
}

/// Deeply nested module: A selectively imports B which wildcard-imports C
/// which defines a trait. The trait from C must still be in type_definitions
/// when generating vtables for B's class.
#[test]
fn test_selective_import_deep_transitive_chain() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.a.svc.{Service}\n",
                    "let s = Service()\n",
                    "println(f\"{s.ping()}\")\n",
                ),
            ),
            (
                "a/svc.mi",
                concat!(
                    "use local.a.traits.health\n",
                    "\n",
                    "public class Service implements Pingable\n",
                    "    fn ping() int: 200\n",
                ),
            ),
            (
                "a/traits/health.mi",
                "trait Pingable\n    fn ping() int\n",
            ),
        ],
        "200",
    );
}

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
