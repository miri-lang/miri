// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a generic body requires of its parameters is answered at every call,
//! whatever the call spells the function as: its bare name, a member of a
//! module alias, or an import alias. The requirement belongs to the function
//! the name resolves to, so a call that renames it still answers for it; a
//! call that did not would compile the body at a type it has no meaning for.

use super::utils::*;

const CALC: (&str, &str) = (
    "calc.mi",
    concat!(
        "fn lt<T>(a T) bool\n",
        "    return a < 10\n",
        "\n",
        "fn combine<T, U>(a T, b U) T\n",
        "    return a + b\n",
    ),
);

const BOXES: (&str, &str) = (
    "boxes.mi",
    concat!(
        "use system.collections.list\n",
        "\n",
        "fn least<T>(xs List<T>) T?\n",
        "    return xs.min()\n",
    ),
);

#[test]
fn a_bare_imported_name_answers_the_bodys_comparison() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.calc.{lt}\n",
                    "\n",
                    "fn main()\n",
                    "    let s = \"a\" + \"b\"\n",
                    "    println(f\"{lt(s)}\")\n",
                ),
            ),
            CALC,
        ],
        "cannot compare String and int",
    );
}

#[test]
fn a_call_through_a_module_alias_answers_the_bodys_comparison() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.calc as C\n",
                    "\n",
                    "fn main()\n",
                    "    let s = \"a\" + \"b\"\n",
                    "    println(f\"{C.lt(3)}\")\n",
                    "    println(f\"{C.lt(s)}\")\n",
                ),
            ),
            CALC,
        ],
        "'lt' applies '<' to its 'T' parameter",
    );
}

#[test]
fn a_call_through_an_import_alias_answers_the_bodys_comparison() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.calc.{lt as L}\n",
                    "\n",
                    "fn main()\n",
                    "    let s = \"a\" + \"b\"\n",
                    "    println(f\"{L(s)}\")\n",
                ),
            ),
            CALC,
        ],
        "'lt' applies '<' to its 'T' parameter",
    );
}

#[test]
fn a_call_through_a_module_alias_answers_the_bodys_arithmetic() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.calc as C\n",
                    "\n",
                    "fn main()\n",
                    "    let x i8 = 5\n",
                    "    let y i64 = 9223372036854775807\n",
                    "    println(f\"{C.combine(x, y)}\")\n",
                ),
            ),
            CALC,
        ],
        "i8 and i64 are not compatible for arithmetic operation",
    );
}

#[test]
fn a_call_through_a_module_alias_answers_the_bodys_ordering_of_elements() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.collections.list\n",
                    "use local.boxes as B\n",
                    "\n",
                    "struct P\n",
                    "    v int\n",
                    "\n",
                    "fn main()\n",
                    "    let xs = List<P>()\n",
                    "    xs.push(P(3))\n",
                    "    match B.least(xs)\n",
                    "        Some(p): println(f\"{p.v}\")\n",
                    "        None: println(\"none\")\n",
                ),
            ),
            BOXES,
        ],
        "MER_TYP_075",
    );
}

#[test]
fn a_call_through_an_import_alias_answers_the_bodys_ordering_of_elements() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.collections.list\n",
                    "use local.boxes.{least as lo}\n",
                    "\n",
                    "struct P\n",
                    "    v int\n",
                    "\n",
                    "fn main()\n",
                    "    let xs = List<P>()\n",
                    "    xs.push(P(3))\n",
                    "    match lo(xs)\n",
                    "        Some(p): println(f\"{p.v}\")\n",
                    "        None: println(\"none\")\n",
                ),
            ),
            BOXES,
        ],
        "MER_TYP_075",
    );
}

#[test]
fn a_call_through_a_module_alias_at_a_type_the_body_supports_runs() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.calc as C\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{C.lt(3)} {C.combine(4, 5)}\")\n",
                ),
            ),
            CALC,
        ],
        "true 9",
    );
}
