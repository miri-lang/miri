// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A private type belongs to the module that declares it.
//!
//! Two modules may each declare a private type of one name, and a program may
//! declare a type of the name a module keeps private: none of them can reach
//! another's, so they are different types and none is refused as a redefinition.

use super::utils::*;
use crate::utils::miri_run_project;

/// The program reaching each module through its alias.
const MAIN_OF_TWO_MODULES: &str = concat!(
    "use local.k.a as a\n",
    "use local.k.b as b\n",
    "\n",
    "fn main()\n",
    "    println(f\"{a.from_a()} {b.from_b()}\")\n",
);

/// A module keeping a private class `Helper` whose `v` answers `value`, and a
/// public function `function` that builds one and returns what it answers.
fn module_with_private_class(function: &str, value: i32) -> String {
    format!(
        "private class Helper\n    fn v() int\n        return {value}\n\n\
         fn {function}() int\n    let h = Helper()\n    return h.v()\n"
    )
}

#[test]
fn test_two_modules_each_keep_a_private_class_of_one_name() {
    let a = module_with_private_class("from_a", 1);
    let b = module_with_private_class("from_b", 2);
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", &a),
            ("k/b.mi", &b),
        ],
        "1 2",
    );
}

#[test]
fn test_a_program_type_shares_the_name_of_a_module_private_type() {
    let a = module_with_private_class("from_a", 1);
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "\n",
                    "class Helper\n",
                    "    fn v() int\n",
                    "        return 9\n",
                    "\n",
                    "fn main()\n",
                    "    let h = Helper()\n",
                    "    println(f\"{a.from_a()} {h.v()}\")\n",
                ),
            ),
            ("k/a.mi", &a),
        ],
        "1 9",
    );
}

#[test]
fn test_two_modules_each_keep_a_private_struct_of_one_name() {
    let a = concat!(
        "private struct Helper\n",
        "    x int\n",
        "\n",
        "fn from_a() String\n",
        "    let h = Helper(x: 4)\n",
        "    return f\"{h.x}\"\n",
    );
    let b = concat!(
        "private struct Helper\n",
        "    name String\n",
        "    y int\n",
        "\n",
        "fn from_b() String\n",
        "    let h = Helper(name: \"bee\", y: 7)\n",
        "    return f\"{h.name}{h.y}\"\n",
    );
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", a),
            ("k/b.mi", b),
        ],
        "4 bee7",
    );
}

/// Each module's assertion renders a failing value by its own struct's fields.
#[test]
fn test_each_module_private_struct_renders_its_own_fields() {
    let a = concat!(
        "use system.testing.{assert_eq}\n",
        "\n",
        "private struct Helper\n",
        "    x int\n",
        "\n",
        "fn check_a()\n",
        "    assert_eq(Helper(x: 4), Helper(x: 4))\n",
        "    println(\"a holds\")\n",
    );
    let b = concat!(
        "use system.testing.{assert_eq}\n",
        "\n",
        "private struct Helper\n",
        "    y int\n",
        "    z int\n",
        "\n",
        "fn check_b()\n",
        "    assert_eq(Helper(y: 1, z: 2), Helper(y: 1, z: 3))\n",
    );
    let result = miri_run_project(&[
        (
            "main.mi",
            "use local.k.a as a\nuse local.k.b as b\n\nfn main()\n    a.check_a()\n    b.check_b()\n",
        ),
        ("k/a.mi", a),
        ("k/b.mi", b),
    ]);
    let output = result.output();
    assert!(!result.success, "b's assertion must fail:\n{output}");
    assert!(output.contains("a holds"), "a's assertion holds:\n{output}");
    assert!(
        output.contains("expected Helper(y=1, z=3), got Helper(y=1, z=2)"),
        "b's values render by b's fields:\n{output}"
    );
}

/// A module keeping a private class whose drop hook prints `line`.
fn module_with_private_drop_hook(function: &str, line: &str) -> String {
    format!(
        "private class Helper\n    public var id int\n\n    public fn drop(self)\n        \
         println(\"{line}\")\n\n\
         fn {function}() int\n    let h = Helper(id: 3)\n    return h.id\n"
    )
}

#[test]
fn test_each_module_private_class_runs_its_own_drop_hook() {
    let a = module_with_private_drop_hook("from_a", "dropped in a");
    let b = module_with_private_drop_hook("from_b", "dropped in b");
    let files = [
        ("main.mi", MAIN_OF_TWO_MODULES),
        ("k/a.mi", a.as_str()),
        ("k/b.mi", b.as_str()),
    ];
    assert_project_runs_with_output(&files, "dropped in a");
    assert_project_runs_with_output(&files, "dropped in b");
    assert_project_runs_with_output(&files, "3 3");
}

#[test]
fn test_a_diagnostic_names_a_module_private_type_by_its_source_name() {
    let a = concat!(
        "private class Helper\n",
        "    fn v() int\n",
        "        return 1\n",
        "\n",
        "fn from_a() int\n",
        "    let h int = Helper()\n",
        "    return h\n",
    );
    let b = module_with_private_class("from_b", 2);
    let result = miri_run_project(&[
        ("main.mi", MAIN_OF_TWO_MODULES),
        ("k/a.mi", a),
        ("k/b.mi", &b),
    ]);
    let output = result.output();
    assert!(!result.success, "the mismatch must be refused:\n{output}");
    assert!(
        output.contains("expected int, got Helper\n"),
        "names the type as its source writes it:\n{output}"
    );
    assert!(!output.contains("k.a.Helper"), "no module path:\n{output}");
    assert!(
        !output.contains("MER_TYP_044"),
        "no redefinition:\n{output}"
    );
}

#[test]
fn test_two_modules_with_a_public_type_of_one_name_are_refused() {
    let public_helper = |function: &str| {
        format!(
            "public class Helper\n    fn v() int\n        return 1\n\n\
             fn {function}() int\n    return Helper().v()\n"
        )
    };
    let a = public_helper("from_a");
    let b = public_helper("from_b");
    assert_project_compiler_error(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", &a),
            ("k/b.mi", &b),
        ],
        "MER_TYP_044",
    );
}

#[test]
fn test_an_importer_cannot_name_a_module_private_type() {
    let a = module_with_private_class("from_a", 1);
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                "use local.k.a\n\nfn main()\n    let h = Helper()\n    println(f\"{h.v()}\")\n",
            ),
            ("k/a.mi", &a),
        ],
        "not visible",
    );
}

/// The program reaching the same module source loaded as `k.a` and `k.b`,
/// where `VALUE` is 1 in the first and 2 in the second.
fn two_copies(source: &str) -> [(String, String); 3] {
    let copy =
        |value: &str, function: &str| source.replace("VALUE", value).replace("from_x", function);
    [
        ("main.mi".to_string(), MAIN_OF_TWO_MODULES.to_string()),
        ("k/a.mi".to_string(), copy("1", "from_a")),
        ("k/b.mi".to_string(), copy("2", "from_b")),
    ]
}

fn assert_two_copies_print(source: &str, expected: &str) {
    let files = two_copies(source);
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    assert_project_runs_with_output(&files, expected);
}

#[test]
fn test_a_module_private_class_takes_and_returns_its_own_type() {
    assert_two_copies_print(
        concat!(
            "private class Helper\n",
            "    var n int\n",
            "    fn init(n int)\n",
            "        self.n = n\n",
            "    fn plus(other Helper) Helper\n",
            "        return Helper(self.n + other.n)\n",
            "\n",
            "fn from_x() int\n",
            "    let h Helper = Helper(VALUE).plus(Helper(100))\n",
            "    return h.n\n",
        ),
        "101 102",
    );
}

#[test]
fn test_a_module_private_class_extends_one_the_module_keeps_private() {
    assert_two_copies_print(
        concat!(
            "private abstract class Base\n",
            "    fn v() int\n",
            "        return 0\n",
            "\n",
            "private class Helper extends Base\n",
            "    fn v() int\n",
            "        return VALUE\n",
            "\n",
            "fn pick(b Base) int\n",
            "    return b.v()\n",
            "\n",
            "fn from_x() int\n",
            "    return pick(Helper())\n",
        ),
        "1 2",
    );
}

#[test]
fn test_a_static_method_of_a_module_private_class() {
    assert_two_copies_print(
        concat!(
            "private class Helper\n",
            "    var n int\n",
            "    static fn make() Helper\n",
            "        return Helper(n: VALUE)\n",
            "\n",
            "fn from_x() int\n",
            "    return Helper.make().n\n",
        ),
        "1 2",
    );
}

#[test]
fn test_a_module_private_class_as_a_collection_element() {
    assert_two_copies_print(
        concat!(
            "use system.collections.list\n",
            "\n",
            "private class Helper\n",
            "    var n int\n",
            "\n",
            "fn from_x() int\n",
            "    var xs List<Helper> = List<Helper>()\n",
            "    xs.push(Helper(n: VALUE))\n",
            "    xs.push(Helper(n: VALUE * 10))\n",
            "    var total = 0\n",
            "    for h in xs\n",
            "        total = total + h.n\n",
            "    return total\n",
        ),
        "11 22",
    );
}

/// A type the module writes is its own private one, never the program's type
/// of the same name: the value is laid out and released as the module's.
#[test]
fn test_a_written_type_names_the_module_private_type_over_the_program_one() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "\n",
                    "class Helper\n",
                    "    var s String\n",
                    "    public fn drop(self)\n",
                    "        println(\"program drop\")\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{a.from_a()}\")\n",
                ),
            ),
            (
                "k/a.mi",
                concat!(
                    "private class Helper\n",
                    "    var n int\n",
                    "    public fn drop(self)\n",
                    "        println(\"module drop\")\n",
                    "\n",
                    "fn take(h Helper) int\n",
                    "    return h.n\n",
                    "\n",
                    "fn from_a() int\n",
                    "    let h Helper = Helper(n: 5)\n",
                    "    return take(h)\n",
                ),
            ),
        ],
        "module drop\n5",
    );
}

/// A generic body is instantiated from the program, and still names the
/// private type of the module that wrote it.
#[test]
fn test_a_generic_body_names_the_private_type_of_its_module() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "use local.k.a.{Box}\n",
                    "\n",
                    "class Helper\n",
                    "    var s String\n",
                    "    fn v() int\n",
                    "        return 99\n",
                    "\n",
                    "fn main()\n",
                    "    let b = Box<String>(item: \"x\")\n",
                    "    let w = a.wrap(\"s\")\n",
                    "    println(f\"{a.wrap(5)} {w} {b.helper_value()}\")\n",
                ),
            ),
            (
                "k/a.mi",
                concat!(
                    "private class Helper\n",
                    "    var n int\n",
                    "    fn v() int\n",
                    "        return self.n\n",
                    "\n",
                    "public fn wrap<T>(x T) int\n",
                    "    let h Helper = Helper(n: 7)\n",
                    "    return h.v()\n",
                    "\n",
                    "public class Box<T>\n",
                    "    var item T\n",
                    "    fn helper_value() int\n",
                    "        let h Helper = Helper(n: 8)\n",
                    "        return h.v()\n",
                ),
            ),
        ],
        "7 7 8",
    );
}
