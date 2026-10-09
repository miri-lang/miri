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

/// A module keeping a private enum `Color` of three variants, one carrying a
/// payload, that builds, matches, renders and compares its own values.
const MODULE_A_COLOR: &str = concat!(
    "private enum Color\n",
    "    Red\n",
    "    Green\n",
    "    Mix(int, int)\n",
    "\n",
    "fn from_a() String\n",
    "    let c = Color.Mix(1, 2)\n",
    "    let n = match c\n",
    "        Color.Red: 10\n",
    "        Color.Green: 20\n",
    "        Color.Mix(x, y): x + y\n",
    "    let same = c == Color.Mix(1, 2)\n",
    "    return f\"a{n} {c} {Color.Green} {same}\"\n",
);

#[test]
fn test_two_modules_each_keep_a_private_enum_of_one_name() {
    let b = concat!(
        "private enum Color\n",
        "    Mix(String)\n",
        "    Blue\n",
        "    Red\n",
        "\n",
        "fn from_b() String\n",
        "    let c = Color.Red\n",
        "    let n = match c\n",
        "        Color.Mix(s): 1\n",
        "        Color.Blue: 2\n",
        "        Color.Red: 3\n",
        "    let m = Color.Mix(\"bee\")\n",
        "    return f\"b{n} {m} {c} {c == Color.Blue}\"\n",
    );
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", MODULE_A_COLOR),
            ("k/b.mi", b),
        ],
        "a3 Mix(1, 2) Green true b3 Mix(bee) Red false",
    );
}

#[test]
fn test_a_program_enum_shares_the_name_of_a_module_private_enum() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "\n",
                    "enum Color\n",
                    "    Up\n",
                    "    Down\n",
                    "\n",
                    "fn main()\n",
                    "    let d = Color.Down\n",
                    "    let n = match d\n",
                    "        Color.Up: 1\n",
                    "        Color.Down: 2\n",
                    "    println(f\"{a.from_a()} {d} {n}\")\n",
                ),
            ),
            ("k/a.mi", MODULE_A_COLOR),
        ],
        "a3 Mix(1, 2) Green true Down 2",
    );
}

/// The exhaustiveness check reads the variants of the module's own enum.
#[test]
fn test_a_match_on_a_module_private_enum_must_cover_its_own_variants() {
    let b = concat!(
        "private enum Color\n",
        "    Blue\n",
        "    Gold\n",
        "\n",
        "fn from_b() String\n",
        "    let c = Color.Blue\n",
        "    let n = match c\n",
        "        Color.Blue: 2\n",
        "    return f\"{n}\"\n",
    );
    assert_project_compiler_error(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", MODULE_A_COLOR),
            ("k/b.mi", b),
        ],
        "Non-exhaustive match on Enum 'Color'. Missing variants: Gold",
    );
}

#[test]
fn test_two_modules_each_keep_a_private_trait_of_one_name() {
    let a = concat!(
        "private trait Shape\n",
        "    fn area() int\n",
        "\n",
        "private class Square implements Shape\n",
        "    var side int\n",
        "    fn area() int\n",
        "        return self.side * self.side\n",
        "\n",
        "fn measure(shape Shape) int\n",
        "    return shape.area()\n",
        "\n",
        "fn from_a() String\n",
        "    let q = Square(side: 3)\n",
        "    return f\"{q.area()} {measure(q)}\"\n",
    );
    let b = concat!(
        "private trait Shape\n",
        "    fn sides() int\n",
        "    fn name() String\n",
        "\n",
        "private class Triangle implements Shape\n",
        "    fn sides() int\n",
        "        return 3\n",
        "    fn name() String\n",
        "        return \"tri\"\n",
        "\n",
        "fn describe(shape Shape) String\n",
        "    return f\"{shape.name()}{shape.sides()}\"\n",
        "\n",
        "fn from_b() String\n",
        "    let t = Triangle()\n",
        "    return f\"{t.sides()} {describe(t)}\"\n",
    );
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", a),
            ("k/b.mi", b),
        ],
        "9 9 3 tri3",
    );
}

#[test]
fn test_two_modules_each_keep_a_private_generic_class_of_one_name() {
    let a = concat!(
        "use system.collections.list\n",
        "\n",
        "private class Box<T>\n",
        "    var item T\n",
        "    fn get() T\n",
        "        return self.item\n",
        "\n",
        "private class Helper\n",
        "    var n int\n",
        "\n",
        "fn from_a() int\n",
        "    let b = Box<int>(item: 4)\n",
        "    var hs = List<Helper>()\n",
        "    hs.push(Helper(n: 10))\n",
        "    return b.get() + hs.get(0).n\n",
    );
    let b = concat!(
        "use system.collections.list\n",
        "\n",
        "private class Box<T>\n",
        "    var count int\n",
        "    var item T\n",
        "    fn get() T\n",
        "        return self.item\n",
        "    fn twice() int\n",
        "        return self.count * 2\n",
        "\n",
        "private class Helper\n",
        "    var label String\n",
        "    var n int\n",
        "\n",
        "fn from_b() int\n",
        "    let b = Box<int>(count: 50, item: 7)\n",
        "    var hs = List<Helper>()\n",
        "    hs.push(Helper(label: \"h\", n: 200))\n",
        "    return b.get() + b.twice() + hs.get(0).n\n",
    );
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN_OF_TWO_MODULES),
            ("k/a.mi", a),
            ("k/b.mi", b),
        ],
        "14 307",
    );
}

/// A type parameter shadows a private type of its module's of the same name.
#[test]
fn test_a_type_parameter_shadows_a_module_private_type_of_its_name() {
    let a = concat!(
        "private class Helper\n",
        "    var n int\n",
        "\n",
        "fn id<Helper>(x Helper) Helper\n",
        "    return x\n",
        "\n",
        "private class Wrap<Helper>\n",
        "    var item Helper\n",
        "    fn get() Helper\n",
        "        return self.item\n",
        "\n",
        "fn from_a() String\n",
        "    let w = Wrap<String>(item: \"w\")\n",
        "    return f\"{id(5)} {w.get()} {Helper(n: 1).n}\"\n",
    );
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                "use local.k.a as a\n\nfn main()\n    println(f\"{a.from_a()}\")\n",
            ),
            ("k/a.mi", a),
        ],
        "5 w 1",
    );
}

#[test]
fn test_a_module_private_enum_has_its_own_methods_and_statics() {
    assert_two_copies_print(
        concat!(
            "use system.collections.list\n",
            "\n",
            "private enum Color\n",
            "    Red\n",
            "    Mix(int)\n",
            "\n",
            "    fn weight() int\n",
            "        return match self\n",
            "            Color.Red: VALUE\n",
            "            Color.Mix(n): n\n",
            "\n",
            "    static fn make() Color\n",
            "        return Color.Mix(VALUE * 100)\n",
            "\n",
            "fn paint(c Color) Color?\n",
            "    return c\n",
            "\n",
            "fn from_x() int\n",
            "    var xs = List<Color>()\n",
            "    xs.push(Color.Red)\n",
            "    xs.push(Color.make())\n",
            "    var total = 0\n",
            "    for c in xs\n",
            "        total = total + c.weight()\n",
            "    let w = match paint(Color.Red)\n",
            "        Some(Color.Red): 1000\n",
            "        Some(Color.Mix(n)): n\n",
            "        None: 0\n",
            "    return total + w\n",
        ),
        "1101 1202",
    );
}

#[test]
fn test_a_module_private_trait_keeps_its_own_default_methods() {
    assert_two_copies_print(
        concat!(
            "private trait Shape\n",
            "    fn base() int\n",
            "    fn scaled() int\n",
            "        return self.base() * VALUE\n",
            "\n",
            "private class Square implements Shape\n",
            "    fn base() int\n",
            "        return 10\n",
            "\n",
            "private trait Named<T>\n",
            "    fn pick(x T) T\n",
            "\n",
            "private class Picker implements Named<int>\n",
            "    fn pick(x int) int\n",
            "        return x + VALUE\n",
            "\n",
            "fn total(shape Shape) int\n",
            "    return shape.scaled()\n",
            "\n",
            "fn from_x() int\n",
            "    let q = Square()\n",
            "    let n Named<int> = Picker()\n",
            "    return total(q) + q.scaled() + n.pick(1000)\n",
        ),
        "1021 1042",
    );
}

/// A module whose public `mk` returns the private `Helper` it keeps.
const MODULE_RETURNING_PRIVATE_HELPER: &str = concat!(
    "private class Helper\n",
    "    var n int\n",
    "    var m int\n",
    "    fn v() int\n",
    "        return self.n + 100\n",
    "\n",
    "public fn mk() Helper\n",
    "    return Helper(n: 1, m: 2)\n",
    "\n",
    "public fn take(h Helper) int\n",
    "    return h.n\n",
);

/// A program declaring its own `Helper`, laid out unlike the module's.
fn program_with_own_helper(body: &str) -> String {
    format!(
        "use local.k.a as a\n\nclass Helper\n    var s String\n    fn v() int\n        return 7\n\n\
         fn main()\n{body}"
    )
}

/// Assert `files` is refused because `name` is a type another module keeps
/// private, and that the refusal never names the type's identity.
fn assert_refused_as_not_visible(files: &[(&str, &str)], name: &str) {
    let result = miri_run_project(files);
    let output = result.output();
    assert!(!result.success, "a private type must not escape:\n{output}");
    assert!(
        output.contains(&format!("Type '{name}' is not visible")),
        "refused as not visible:\n{output}"
    );
    assert!(!output.contains("local.k."), "no identity in:\n{output}");
}

#[test]
fn test_a_private_return_type_is_refused_beside_a_program_type_of_its_name() {
    let main = program_with_own_helper("    let h = a.mk()\n    println(f\"{h.v()}\")\n");
    let files = [
        ("main.mi", main.as_str()),
        ("k/a.mi", MODULE_RETURNING_PRIVATE_HELPER),
    ];
    assert_refused_as_not_visible(&files, "Helper");
    let output = miri_run_project(&files).output();
    assert!(
        output.contains("main.mi:9:13"),
        "refused at the call:\n{output}"
    );
}

#[test]
fn test_a_private_parameter_type_is_refused_beside_a_program_type_of_its_name() {
    let main =
        program_with_own_helper("    let h = Helper(s: \"x\")\n    println(f\"{a.take(h)}\")\n");
    assert_refused_as_not_visible(
        &[
            ("main.mi", &main),
            ("k/a.mi", MODULE_RETURNING_PRIVATE_HELPER),
        ],
        "Helper",
    );
}

#[test]
fn test_a_private_return_type_is_refused_where_no_type_shares_its_name() {
    assert_refused_as_not_visible(
        &[
            (
                "main.mi",
                "use local.k.a as a\n\nfn main()\n    let h = a.mk()\n    println(f\"{h.v()}\")\n",
            ),
            ("k/a.mi", MODULE_RETURNING_PRIVATE_HELPER),
        ],
        "Helper",
    );
}

#[test]
fn test_a_private_enum_return_type_is_refused_beside_a_program_enum_of_its_name() {
    assert_refused_as_not_visible(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "\n",
                    "enum Kind\n",
                    "    Big(String, String)\n",
                    "    Small\n",
                    "\n",
                    "fn main()\n",
                    "    let z = a.make()\n",
                    "    let r = match z\n",
                    "        Kind.Big(p, q): f\"{p}{q}\"\n",
                    "        Kind.Small: \"s\"\n",
                    "    println(r)\n",
                ),
            ),
            (
                "k/a.mi",
                concat!(
                    "private enum Kind\n",
                    "    Num(int)\n",
                    "    Text(String)\n",
                    "\n",
                    "public fn make() Kind\n",
                    "    return Kind.Num(41)\n",
                ),
            ),
        ],
        "Kind",
    );
}

#[test]
fn test_a_private_return_type_is_refused_in_a_module_keeping_a_type_of_its_name() {
    assert_refused_as_not_visible(
        &[
            (
                "main.mi",
                "use local.k.a as a\n\nfn main()\n    println(f\"{a.go()}\")\n",
            ),
            (
                "k/a.mi",
                concat!(
                    "use local.k.b as b\n",
                    "\n",
                    "private class Helper\n",
                    "    var s String\n",
                    "    fn v() int\n",
                    "        return 7\n",
                    "\n",
                    "public fn go() int\n",
                    "    let h = b.mk()\n",
                    "    return h.v()\n",
                ),
            ),
            ("k/b.mi", MODULE_RETURNING_PRIVATE_HELPER),
        ],
        "Helper",
    );
}

#[test]
fn test_a_private_type_argument_of_a_return_type_is_refused() {
    let main = program_with_own_helper("    let b = a.boxed()\n    println(f\"{b.get().v()}\")\n")
        .replacen(
            "use local.k.a as a\n",
            "use local.k.a as a\nuse local.k.box.{Box}\n",
            1,
        );
    assert_refused_as_not_visible(
        &[
            ("main.mi", &main),
            (
                "k/box.mi",
                "public class Box<T>\n    var item T\n    fn get() T\n        return self.item\n",
            ),
            (
                "k/a.mi",
                concat!(
                    "use local.k.box.{Box}\n",
                    "\n",
                    "private class Helper\n",
                    "    var n int\n",
                    "    fn v() int\n",
                    "        return self.n\n",
                    "\n",
                    "public fn boxed() Box<Helper>\n",
                    "    return Box<Helper>(item: Helper(n: 1))\n",
                ),
            ),
        ],
        "Helper",
    );
}

/// The program's own type is reached by its own name, wherever a module
/// keeps a private type of the same name.
#[test]
fn test_a_program_type_names_itself_beside_a_module_private_type_of_its_name() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "\n",
                    "class Helper\n",
                    "    var s String\n",
                    "    fn me() Helper\n",
                    "        return self\n",
                    "\n",
                    "fn main()\n",
                    "    let h = Helper(s: \"p\")\n",
                    "    println(f\"{h.me().s} {a.from_a()}\")\n",
                ),
            ),
            ("k/a.mi", &module_with_private_class("from_a", 1)),
        ],
        "p 1",
    );
}

#[test]
fn test_a_nested_module_and_a_sibling_each_keep_a_private_class_of_one_name() {
    let a = module_with_private_class("from_a", 1);
    let b = module_with_private_class("from_b", 2);
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.sub.a as a\n",
                    "use local.k.b as b\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{a.from_a()} {b.from_b()}\")\n",
                ),
            ),
            ("k/sub/a.mi", &a),
            ("k/b.mi", &b),
        ],
        "1 2",
    );
}

#[test]
fn test_a_module_private_class_names_itself_as_self() {
    assert_two_copies_print(
        concat!(
            "private class Helper\n",
            "    var n int\n",
            "    fn plus(other Self) Self\n",
            "        return Helper(n: self.n + other.n)\n",
            "\n",
            "fn from_x() int\n",
            "    return Helper(n: VALUE).plus(Helper(n: 10)).n\n",
        ),
        "11 12",
    );
}

#[test]
fn test_two_private_types_of_a_module_name_each_other() {
    assert_two_copies_print(
        concat!(
            "private class Node\n",
            "    var tag Tag\n",
            "    fn weight() int\n",
            "        return self.tag.of(self)\n",
            "\n",
            "private class Tag\n",
            "    var n int\n",
            "    fn of(node Node) int\n",
            "        return self.n * VALUE\n",
            "\n",
            "fn from_x() int\n",
            "    return Node(tag: Tag(n: 5)).weight()\n",
        ),
        "5 10",
    );
}

/// Each diagnostic about a type a module keeps private names it as the module
/// writes it.
#[test]
fn test_diagnostics_name_a_module_private_type_by_its_source_name() {
    let cases: [(&str, &str, &str); 6] = [
        (
            "MER_TYP_057",
            concat!(
                "private trait Shape\n",
                "    fn area() int\n",
                "private class Tri implements Shape\n",
                "    fn sides() int\n",
                "        return 3\n",
            ),
            "Class 'Tri' must implement method 'area' from trait 'Shape'",
        ),
        (
            "MER_TYP_033",
            concat!(
                "private class Helper\n",
                "    var n int\n",
                "fn from_a() int\n",
                "    return Helper(n: 1).missing\n",
            ),
            "'Helper'",
        ),
        (
            "MER_TYP_056",
            concat!(
                "private abstract class Shape\n",
                "    fn area() int\n",
                "fn from_a() int\n",
                "    let s = Shape()\n",
                "    return 1\n",
            ),
            "'Shape'",
        ),
        (
            "MER_TYP_038",
            concat!(
                "private enum Color\n",
                "    Red\n",
                "fn from_a() int\n",
                "    let c = Color.Blue\n",
                "    return 1\n",
            ),
            "'Color'",
        ),
        (
            "MER_TYP_057",
            concat!(
                "private trait Shape\n",
                "    fn area() int\n",
                "private class Tri implements Shape\n",
                "    fn area() String\n",
                "        return \"x\"\n",
            ),
            "'Tri'",
        ),
        (
            "MER_TYP_057",
            concat!(
                "private class Part\n",
                "    var n int\n",
                "private trait Shape\n",
                "    fn take(p Part) int\n",
                "private class Tri implements Shape\n",
                "    fn take(p int) int\n",
                "        return p\n",
            ),
            "fn take(p: Part) -> int",
        ),
    ];
    for (code, module, expected) in cases {
        let result = miri_run_project(&[
            (
                "main.mi",
                "use local.k.a as a\n\nfn main()\n    println(\"x\")\n",
            ),
            ("k/a.mi", module),
        ]);
        let output = result.output();
        assert!(output.contains(code), "{code} expected:\n{output}");
        assert!(
            output.contains(expected),
            "{code} names '{expected}':\n{output}"
        );
        assert!(
            !output.contains("local.k."),
            "{code} leaks an identity:\n{output}"
        );
    }
}

/// A type parameter bound through a closure type a module wrote is bound to
/// the module's private type, never to the program's type of its name.
#[test]
fn test_a_closure_type_binds_a_type_parameter_to_the_module_private_type() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.k.a as a\n",
                    "\n",
                    "class Helper\n",
                    "    var s String\n",
                    "    var t String\n",
                    "    public fn drop(self)\n",
                    "        println(\"program drop\")\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{a.from_a()}\")\n",
                ),
            ),
            (
                "k/s.mi",
                "public fn call<T>(f fn() T) int\n    let v = f()\n    return 1\n",
            ),
            (
                "k/a.mi",
                concat!(
                    "use local.k.s.{call}\n",
                    "\n",
                    "private class Helper\n",
                    "    var n int\n",
                    "    public fn drop(self)\n",
                    "        println(\"module drop\")\n",
                    "\n",
                    "fn mk() Helper\n",
                    "    return Helper(n: 6)\n",
                    "\n",
                    "public fn from_a() int\n",
                    "    let g = fn() Helper: Helper(n: 3)\n",
                    "    return call(mk) + call(g)\n",
                ),
            ),
        ],
        "module drop\nmodule drop\n2",
    );
}

/// A public class of a module holding the private `Helper` it keeps.
const MODULE_WITH_PRIVATE_MEMBER: &str = concat!(
    "private class Helper\n",
    "    var n int\n",
    "    fn v() int\n",
    "        return self.n + 100\n",
    "\n",
    "public class Shell\n",
    "    public var h Helper\n",
    "    public fn get() Helper\n",
    "        return self.h\n",
    "\n",
    "public fn mk() Shell\n",
    "    return Shell(h: Helper(n: 1))\n",
);

/// A program reading the member `member` of the module's `Shell`.
fn program_reading(member: &str) -> String {
    program_with_own_helper(&format!(
        "    let s = a.mk()\n    let x = s.{member}\n    println(f\"{{x.v()}}\")\n"
    ))
    .replacen(
        "use local.k.a as a\n",
        "use local.k.a as a\nuse local.k.a.{Shell}\n",
        1,
    )
}

#[test]
fn test_a_private_field_type_is_refused_where_the_importer_reads_it() {
    let main = program_reading("h");
    assert_refused_as_not_visible(
        &[("main.mi", &main), ("k/a.mi", MODULE_WITH_PRIVATE_MEMBER)],
        "Helper",
    );
}

#[test]
fn test_a_private_method_return_type_is_refused_at_the_importer_call() {
    let main = program_reading("get()");
    let files = [
        ("main.mi", main.as_str()),
        ("k/a.mi", MODULE_WITH_PRIVATE_MEMBER),
    ];
    assert_refused_as_not_visible(&files, "Helper");
    let output = miri_run_project(&files).output();
    assert!(
        output.contains("main.mi:11:"),
        "refused at the call:\n{output}"
    );
}

/// A local binding shadows a private type of its name, as it shadows a public
/// one.
#[test]
fn test_a_local_binding_shadows_a_module_private_type_of_its_name() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                "use local.k.a as a\n\nfn main()\n    println(f\"{a.f()}\")\n",
            ),
            (
                "k/a.mi",
                concat!(
                    "private class Helper\n",
                    "    fn v() int\n",
                    "        return 1\n",
                    "\n",
                    "public fn f() int\n",
                    "    let Helper = 5\n",
                    "    return Helper + 1\n",
                ),
            ),
        ],
        "6",
    );
}

/// The module's private `Helper` and a public `opt` handing back what `form`
/// spells, built from `value`.
fn module_returning(form: &str, value: &str) -> String {
    format!(
        "private class Helper\n    var n int\n    fn v() int\n        return self.n + 100\n\n\
         public fn opt() {form}\n    return {value}\n"
    )
}

/// A program, beside its own `Helper`, that calls the module's `opt`.
fn program_calling_opt() -> String {
    program_with_own_helper("    let o = a.opt()\n    println(\"called\")\n")
}

#[test]
fn test_a_private_type_is_refused_through_its_nullable_spelling() {
    let cases = [
        ("Helper?", "Helper(n: 1)"),
        ("[Helper?]", "[Helper(n: 1)]"),
        ("(Helper?, int)", "(Helper(n: 1), 2)"),
        ("fn() Helper?", "fn() Helper?: Helper(n: 1)"),
    ];
    for (form, value) in cases {
        let module = module_returning(form, value);
        let main = program_calling_opt();
        assert_refused_as_not_visible(&[("main.mi", &main), ("k/a.mi", &module)], "Helper");
    }
}

#[test]
fn test_a_private_nullable_parameter_type_is_refused() {
    let module = concat!(
        "private class Helper\n",
        "    var n int\n",
        "\n",
        "public fn take(h Helper?) int\n",
        "    return 1\n",
    );
    let main = program_with_own_helper(
        "    let h Helper? = Helper(s: \"x\")\n    println(f\"{a.take(h)}\")\n",
    );
    assert_refused_as_not_visible(&[("main.mi", &main), ("k/a.mi", module)], "Helper");
}

#[test]
fn test_a_closure_type_naming_a_module_private_type_is_its_own_type() {
    assert_two_copies_print(
        concat!(
            "private class Helper\n",
            "    var n int\n",
            "\n",
            "fn mkg() fn(Helper) int\n",
            "    return fn(h Helper) int: h.n\n",
            "\n",
            "fn apply(g fn(Helper) int) int\n",
            "    return g(Helper(n: VALUE))\n",
            "\n",
            "fn from_x() int\n",
            "    let g = mkg()\n",
            "    return g(Helper(n: 5)) + apply(g)\n",
        ),
        "6 7",
    );
}

#[test]
fn test_a_private_type_bound_by_a_pattern_is_refused() {
    let main = program_with_own_helper(concat!(
        "    let e = a.mk()\n",
        "    match e\n",
        "        E.One(h): println(f\"{h.v()}\")\n",
        "        E.Two: println(\"two\")\n",
    ))
    .replacen(
        "use local.k.a as a\n",
        "use local.k.a as a\nuse local.k.a.{E}\n",
        1,
    );
    let module = concat!(
        "private class Helper\n",
        "    var n int\n",
        "    fn v() int\n",
        "        return self.n + 100\n",
        "\n",
        "public enum E\n",
        "    One(Helper)\n",
        "    Two\n",
        "\n",
        "public fn mk() E\n",
        "    return E.One(Helper(n: 1))\n",
    );
    assert_refused_as_not_visible(&[("main.mi", &main), ("k/a.mi", module)], "Helper");
}

#[test]
fn test_a_private_field_of_a_public_struct_is_refused_where_read() {
    let main =
        program_with_own_helper("    let p = a.mk()\n    let x = p.h\n    println(f\"{x.v()}\")\n")
            .replacen(
                "use local.k.a as a\n",
                "use local.k.a as a\nuse local.k.a.{Pack}\n",
                1,
            );
    let module = concat!(
        "private class Helper\n",
        "    var n int\n",
        "    fn v() int\n",
        "        return self.n + 100\n",
        "\n",
        "public struct Pack\n",
        "    h Helper\n",
        "\n",
        "public fn mk() Pack\n",
        "    return Pack(h: Helper(n: 1))\n",
    );
    assert_refused_as_not_visible(&[("main.mi", &main), ("k/a.mi", module)], "Helper");
}
