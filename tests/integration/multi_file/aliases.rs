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

/// An argument built at run time and handed to a function through a module
/// alias is released once the call has its own reference, as a direct call's
/// is; named arguments through the alias bind by name.
#[test]
fn test_module_alias_call_releases_a_built_argument_and_binds_named_ones() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.boxes as B\n",
                    "\n",
                    "fn main()\n",
                    "    let b = B.wrap_s(\"a\" + \"b\")\n",
                    "    println(b.put(\"c\" + \"d\"))\n",
                    "    println(B.pair(right: \"r\" + \"1\", left: \"l\" + \"1\"))\n",
                ),
            ),
            (
                "utils/boxes.mi",
                concat!(
                    "public class Box<T>\n",
                    "    public var v T\n",
                    "    public fn put(x T) T\n",
                    "        self.v = x\n",
                    "        return x\n",
                    "\n",
                    "public fn wrap_s(s String) Box<String>\n",
                    "    return Box<String>(v: s)\n",
                    "\n",
                    "public fn pair(left String, right String) String\n",
                    "    return f\"{left}|{right}\"\n",
                ),
            ),
        ],
        "cd\nl1|r1",
    );
}

/// A module alias with the name of a type the program declares would shadow
/// it silently, since `M.f` reads the alias first: the clash is refused,
/// naming both.
#[test]
fn test_module_alias_named_like_a_declared_type_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.math as M

class M
    fn sqrt(x f64) f64
        return 99.0

fn main()
    println(f"{M.sqrt(4.0)}")
"#,
        "Module alias 'M' for 'system.math' has the name of the type 'M'",
    );
}

/// A member read through an alias reaches the module's declaration, even when
/// the program declares a function of the same name.
#[test]
fn test_module_alias_reaches_the_module_function_over_a_same_named_program_function() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.m6 as S\n",
                    "\n",
                    "fn lt<T>(a T) bool\n",
                    "    return a < 10\n",
                    "\n",
                    "fn main()\n",
                    "    println(f'{S.lt(5)} {lt(5)}')\n",
                ),
            ),
            (
                "m6.mi",
                concat!("fn lt<T>(a T) bool\n", "    return false\n",),
            ),
        ],
        "false true",
    );
}

/// Two modules declaring one name can each be imported under an alias, and
/// each alias reaches its own module's function.
#[test]
fn test_two_aliased_modules_may_declare_the_same_name() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.m7 as A\n",
                    "use local.m8 as B\n",
                    "\n",
                    "fn main()\n",
                    "    println(f'{A.helper()} {B.helper()}')\n",
                ),
            ),
            ("m7.mi", concat!("fn helper() int\n", "    return 7\n",)),
            ("m8.mi", concat!("fn helper() int\n", "    return 8\n",)),
        ],
        "7 8",
    );
}

/// A module's private function shares no name with the program: declared
/// before or after the `use`, the program's own function is the one called.
#[test]
fn test_a_modules_private_function_does_not_replace_the_programs() {
    let module = (
        "m10.mi",
        concat!(
            "private fn helper() int\n",
            "    return 10\n",
            "\n",
            "public fn wrapped() int\n",
            "    return helper()\n",
        ),
    );
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "fn helper() int\n",
                    "    return 1\n",
                    "\n",
                    "use local.m10\n",
                    "\n",
                    "fn main()\n",
                    "    println(f'{helper()} {wrapped()}')\n",
                ),
            ),
            module,
        ],
        "1 10",
    );
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.m10\n",
                    "\n",
                    "fn helper() int\n",
                    "    return 1\n",
                    "\n",
                    "fn main()\n",
                    "    println(f'{helper()} {wrapped()}')\n",
                ),
            ),
            module,
        ],
        "1 10",
    );
}

/// Each function's residency verdict is its own: a module's host-only `len_of`
/// does not make the program's residency-polymorphic `len_of` refuse a
/// gpu-resident argument.
#[test]
fn test_same_named_functions_keep_their_own_residency() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.gpu\n",
                    "use system.collections.array\n",
                    "\n",
                    "fn len_of(a Array<int,4>) int\n",
                    "    return a.length()\n",
                    "\n",
                    "use local.dbg\n",
                    "\n",
                    "fn main()\n",
                    "    gpu var g = [1, 2, 3, 4]\n",
                    "    println(f'{len_of(g)} {first([5, 6, 7, 8])}')\n",
                ),
            ),
            (
                "dbg.mi",
                concat!(
                    "use system.collections.array\n",
                    "\n",
                    "private fn len_of(a Array<int,4>) int\n",
                    "    println(f'{a[0]}')\n",
                    "    return 0\n",
                    "\n",
                    "public fn first(a Array<int,4>) int\n",
                    "    return len_of(a)\n",
                ),
            ),
        ],
        "4 0",
    );
}

/// A function read through a module alias as a value is the same function
/// value its bare name is.
#[test]
fn test_a_function_read_through_a_module_alias_is_a_value() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.helper as helper\n",
                    "\n",
                    "fn apply(f fn(int) int, x int) int\n",
                    "    return f(x)\n",
                    "\n",
                    "fn main()\n",
                    "    let g = helper.inc\n",
                    "    println(f'{g(9)} {apply(helper.inc, 1)}')\n",
                ),
            ),
            (
                "helper.mi",
                concat!("public fn inc(x int) int\n", "    return x + 1\n",),
            ),
        ],
        "10 2",
    );
}

/// A module already loaded by one import keeps the aliases a later import
/// gives its items.
#[test]
fn test_an_already_loaded_module_keeps_a_later_item_alias() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.api.{api}\n",
                    "use local.deep.{deep}\n",
                    "\n",
                    "fn main()\n",
                    "    println(f'{api()} {deep()}')\n",
                ),
            ),
            (
                "api.mi",
                concat!("public fn api() int\n", "    return 50\n",),
            ),
            (
                "deep.mi",
                concat!(
                    "use local.api.{api as mapi}\n",
                    "\n",
                    "public fn deep() int\n",
                    "    return mapi() + 7\n",
                ),
            ),
        ],
        "50 57",
    );
}

/// Two modules' items of one name can each be imported under an alias of its
/// own, as the conflict's help recommends.
#[test]
fn test_same_named_items_imported_under_two_aliases() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.x.{gen as xgen}\n",
                    "use local.y.{gen as ygen}\n",
                    "\n",
                    "fn main()\n",
                    "    println(f'{xgen()} {ygen()}')\n",
                ),
            ),
            ("x.mi", concat!("public fn gen() int\n", "    return 1\n",)),
            ("y.mi", concat!("public fn gen() int\n", "    return 2\n",)),
        ],
        "1 2",
    );
}

/// A module reached a second time through a symlinked path keeps the alias
/// that import gives its item.
#[cfg(unix)]
#[test]
fn test_a_module_reached_through_a_second_path_keeps_its_alias() {
    let dir = tempfile::tempdir().expect("a temporary project directory");
    let real = dir.path().join("real");
    std::fs::create_dir(&real).expect("the module directory");
    std::fs::write(real.join("m.mi"), "public fn bump() int\n    return 3\n").expect("the module");
    std::os::unix::fs::symlink(&real, dir.path().join("link")).expect("the second path");
    let main = dir.path().join("main.mi");
    std::fs::write(
        &main,
        "use local.real.m.{bump as b1}\nuse local.link.m.{bump as b2}\n\nfn main()\n    println(f'{b1()} {b2()}')\n",
    )
    .expect("the program");
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib");
    let output = crate::utils::miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib)
        .current_dir(dir.path())
        .arg("run")
        .arg(&main)
        .output()
        .expect("run miri");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("3 3"),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
