// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter named after a type its declaring module sees hides that
//! type for the whole declaration, and is refused where it is declared
//! (`MER_TYP_064`).
//!
//! What a module sees is decided from its own scope: the types it declares,
//! those its `use` lines bring in, the prelude, and the built-in spellings. A
//! type declared somewhere the module cannot reach is not hidden by a
//! parameter of its name, so the parameter is accepted.

use super::utils::*;

/// One project and what checking it must say: `Some(message)` for a refusal
/// carrying `message`, `None` for a project that type-checks with no
/// `MER_TYP_064` at all.
struct Case {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    refusal: Option<&'static str>,
}

/// The entry file of a project whose module `k/a.mi` declares `from_a`.
const MAIN_USING_A: &str = concat!(
    "use local.k.a as a\n",
    "\n",
    "fn main()\n",
    "    println(f\"{a.from_a()}\")\n",
);

/// A module declaring a public class `Widget`.
const WIDGET_MODULE: &str = "public class Widget\n    var n int\n";

/// A module with a generic function whose parameter is called `Widget`, and
/// `from_b` calling it.
const B_WITH_WIDGET_PARAMETER: &str = concat!(
    "fn id<Widget>(x Widget) Widget\n",
    "    return x\n",
    "\n",
    "fn from_b() int\n",
    "    return id(5)\n",
);

/// Checks every case, then fails once naming each case whose check said
/// something other than what the case expects.
fn check_cases(cases: &[Case]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let report = check_project_report(case.files);
            let refused = report.contains("MER_TYP_064");
            let unexpected = match case.refusal {
                Some(message) => !report.contains(message),
                None => refused || report.contains("error["),
            };
            unexpected.then(|| format!("--- {}:\n{report}", case.name))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_kind_of_type_in_scope_is_hidden_by_a_parameter_of_its_name() {
    check_cases(&[
        Case {
            name: "trait",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "fn f<Named>(x Named) Named\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "class",
            files: &[(
                "main.mi",
                concat!(
                    "class Point\n    var x int\n\n",
                    "fn f<Point>(x Point) Point\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Point` hides the class `Point`"),
        },
        Case {
            name: "struct",
            files: &[(
                "main.mi",
                concat!(
                    "struct Pair\n    a int\n\n",
                    "fn f<Pair>(x Pair) Pair\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Pair` hides the struct `Pair`"),
        },
        Case {
            name: "enum",
            files: &[(
                "main.mi",
                concat!(
                    "enum Color\n    Red\n    Green\n\n",
                    "fn f<Color>(x Color) Color\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Color` hides the enum `Color`"),
        },
        Case {
            name: "alias",
            files: &[(
                "main.mi",
                concat!(
                    "type Num is int\n\n",
                    "fn f<Num>(x Num) Num\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Num` hides the type `Num`"),
        },
        Case {
            name: "type set",
            files: &[(
                "main.mi",
                concat!(
                    "type Real is f32 or float\n\n",
                    "fn f<Real>(x Real) Real\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Real` hides the type `Real`"),
        },
        Case {
            name: "String",
            files: &[(
                "main.mi",
                concat!(
                    "fn f<String>(x String) String\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `String` hides the class `String`"),
        },
        Case {
            name: "List",
            files: &[(
                "main.mi",
                concat!(
                    "use system.collections.list\n\n",
                    "fn f<List>(x List) List\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `List` hides the class `List`"),
        },
        Case {
            name: "Option",
            files: &[(
                "main.mi",
                concat!(
                    "fn f<Option>(x Option) Option\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Option` hides the type `Option`"),
        },
        Case {
            name: "Option in a struct",
            files: &[(
                "main.mi",
                concat!(
                    "struct Pair<Option, V>\n    a Option\n    b V\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Option` hides the type `Option`"),
        },
        Case {
            name: "a type the program imports",
            files: &[
                (
                    "main.mi",
                    concat!(
                        "use local.k.w\n\n",
                        "fn f<Widget>(x Widget) Widget\n    return x\n\n",
                        "fn main()\n    println(\"x\")\n",
                    ),
                ),
                ("k/w.mi", WIDGET_MODULE),
            ],
            refusal: Some("type parameter `Widget` hides the class `Widget`"),
        },
        Case {
            name: "a type a module imports",
            files: &[
                ("main.mi", MAIN_USING_A),
                (
                    "k/a.mi",
                    concat!(
                        "use local.k.w\n\n",
                        "fn id<Widget>(x Widget) Widget\n    return x\n\n",
                        "fn from_a() int\n    return id(5)\n",
                    ),
                ),
                ("k/w.mi", WIDGET_MODULE),
            ],
            refusal: Some("type parameter `Widget` hides the class `Widget`"),
        },
        Case {
            name: "a type a module selects from an import",
            files: &[
                ("main.mi", MAIN_USING_A),
                (
                    "k/a.mi",
                    concat!(
                        "use local.k.w.{Widget}\n\n",
                        "fn id<Widget>(x Widget) Widget\n    return x\n\n",
                        "fn from_a() int\n    return id(5)\n",
                    ),
                ),
                ("k/w.mi", WIDGET_MODULE),
            ],
            refusal: Some("type parameter `Widget` hides the class `Widget`"),
        },
        Case {
            name: "a type a module declares",
            files: &[
                ("main.mi", MAIN_USING_A),
                (
                    "k/a.mi",
                    concat!(
                        "public class Helper\n    var n int\n\n",
                        "fn id<Helper>(x Helper) Helper\n    return x\n\n",
                        "fn from_a() int\n    return id(5)\n",
                    ),
                ),
            ],
            refusal: Some("type parameter `Helper` hides the class `Helper`"),
        },
        Case {
            name: "an alias a module declares",
            files: &[
                ("main.mi", MAIN_USING_A),
                (
                    "k/a.mi",
                    concat!(
                        "type Count is int\n\n",
                        "fn id<Count>(x Count) Count\n    return x\n\n",
                        "fn from_a() int\n    return id(5)\n",
                    ),
                ),
            ],
            refusal: Some("type parameter `Count` hides the type `Count`"),
        },
        Case {
            name: "a prelude type in a module",
            files: &[
                ("main.mi", MAIN_USING_A),
                (
                    "k/a.mi",
                    concat!(
                        "fn id<String>(x String) String\n    return x\n\n",
                        "fn from_a() int\n    return 5\n",
                    ),
                ),
            ],
            refusal: Some("type parameter `String` hides the class `String`"),
        },
    ]);
}

#[test]
fn every_kind_of_declaration_refuses_a_parameter_hiding_a_type() {
    check_cases(&[
        Case {
            name: "function",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "fn f<Named>(x Named) Named\n    return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "class",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "class Box<Named>\n    var v Named\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "struct",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "struct Pair<Named, V>\n    a Named\n    b V\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "enum",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "enum Maybe<Named>\n    Just(Named)\n    Nothing\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "trait method without a body",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "trait Gen\n    fn m<Named>(x Named) Named\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "trait method with a body",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "trait Gen\n    fn m<Named>(x Named) Named\n        return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
        Case {
            name: "class method",
            files: &[(
                "main.mi",
                concat!(
                    "trait Named\n    fn name() String\n\n",
                    "class C\n    fn m<Named>(x Named) Named\n        return x\n\n",
                    "fn main()\n    println(\"x\")\n",
                ),
            )],
            refusal: Some("type parameter `Named` hides the trait `Named`"),
        },
    ]);
}

#[test]
fn a_parameter_named_after_a_type_its_module_cannot_see_is_accepted() {
    check_cases(&[
        Case {
            name: "a class the program declares, in a module",
            files: &[
                (
                    "main.mi",
                    concat!(
                        "use local.k.b as b\n\n",
                        "class Widget\n    var n int\n\n",
                        "fn main()\n    println(f\"{b.from_b()}\")\n",
                    ),
                ),
                ("k/b.mi", B_WITH_WIDGET_PARAMETER),
            ],
            refusal: None,
        },
        Case {
            name: "a class the program imports and the module does not",
            files: &[
                (
                    "main.mi",
                    concat!(
                        "use local.k.w\n",
                        "use local.k.b as b\n\n",
                        "fn main()\n    let w = Widget(1)\n    println(f\"{b.from_b()} {w.n}\")\n",
                    ),
                ),
                ("k/b.mi", B_WITH_WIDGET_PARAMETER),
                ("k/w.mi", WIDGET_MODULE),
            ],
            refusal: None,
        },
        Case {
            name: "an alias the program declares, in a module",
            files: &[
                (
                    "main.mi",
                    concat!(
                        "use local.k.b as b\n\n",
                        "type Widget is int\n\n",
                        "fn main()\n    println(f\"{b.from_b()}\")\n",
                    ),
                ),
                ("k/b.mi", B_WITH_WIDGET_PARAMETER),
            ],
            refusal: None,
        },
        Case {
            name: "a class another module keeps private, in the program",
            files: &[
                (
                    "main.mi",
                    concat!(
                        "use local.k.b as b\n\n",
                        "fn id<Widget>(x Widget) Widget\n    return x\n\n",
                        "fn main()\n    println(f\"{b.from_b()} {id(1)}\")\n",
                    ),
                ),
                (
                    "k/b.mi",
                    concat!(
                        "private class Widget\n    var n int\n\n",
                        "fn from_b() int\n    return Widget(5).n\n",
                    ),
                ),
            ],
            refusal: None,
        },
        Case {
            name: "program types named like the parameters of stdlib generics",
            files: &[(
                "main.mi",
                concat!(
                    "use system.collections.list\n",
                    "use system.collections.map\n\n",
                    "enum E\n    One\n\n",
                    "enum T\n    Two\n\n",
                    "class K\n    var n int\n\n",
                    "class V\n    var n int\n\n",
                    "fn parse(s String) Result<int, String>\n",
                    "    return Result.Ok(s.length())\n\n",
                    "fn main()\n",
                    "    let xs = List([1, 2])\n",
                    "    var m = Map<String, int>()\n",
                    "    m[\"a\"] = 1\n",
                    "    let n = parse(\"abc\").unwrap_or(0)\n",
                    "    println(f\"{xs.length()} {m.length()} {n}\")\n",
                ),
            )],
            refusal: None,
        },
    ]);
}
