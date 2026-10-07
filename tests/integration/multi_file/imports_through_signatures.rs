// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Imports a file needs without ever spelling them.
//!
//! A file that calls `rows()` and reads a field off an element never writes the
//! element's type, and yet the import that brings the type in is what makes the
//! callee's declared return type resolvable here. Reporting that import as
//! unused, and telling the reader to delete the line it shares with the
//! function they do call, talks a reader into breaking a build that worked.

use super::utils::*;

/// A module declaring a class and a function whose return type names it, plus a
/// class no declaration in it mentions.
const LIB: &str = concat!(
    "use system.collections.list\n",
    "\n",
    "public class Tally\n",
    "    public var total int\n",
    "\n",
    "    public fn init(total int)\n",
    "        self.total = total\n",
    "\n",
    "public class WordCount\n",
    "    public var count int\n",
    "\n",
    "    public fn init(count int)\n",
    "        self.count = count\n",
    "\n",
    "public class Report\n",
    "    public fn init()\n",
    "        self.rows()\n",
    "\n",
    "    public fn rows() [WordCount]\n",
    "        return List([WordCount(3)])\n",
    "\n",
    "public fn rows() [WordCount]\n",
    "    return List([WordCount(3), WordCount(4)])\n",
);

/// A name reached only through the declared type of a function the file calls
/// is a name the file needs: dropping the import makes the program stop
/// compiling, so it is not spare.
#[test]
fn test_a_type_reached_through_a_callees_return_is_not_unused() {
    assert_project_no_warning(
        &[
            (
                "main.mi",
                concat!(
                    "use local.lib.{rows, WordCount}\n",
                    "\n",
                    "fn main()\n",
                    "    let r = rows()\n",
                    "    let first = r[0]\n",
                    "    println(f\"{first.count}\")\n",
                ),
            ),
            ("lib.mi", LIB),
        ],
        "MER_IMP_005",
    );
}

/// A selective import whose other names are read has one name to drop, not a
/// line to delete: deleting the line takes the function the file calls with it.
#[test]
fn test_an_unused_selected_name_says_which_symbol_to_drop() {
    let output = check_project(&[
        (
            "main.mi",
            concat!(
                "use local.lib.{rows, Tally}\n",
                "\n",
                "fn main()\n",
                "    let r = rows()\n",
                "    println(f\"{r.length()}\")\n",
            ),
        ),
        ("lib.mi", LIB),
    ]);

    assert!(
        output.contains("MER_IMP_005"),
        "'Tally' is read nowhere here, through a signature or otherwise:\n{}",
        output
    );
    assert!(
        output.contains("drop 'Tally' from the selection"),
        "the help must name the symbol to drop:\n{}",
        output
    );
    assert!(
        !output.contains("remove the line"),
        "removing the line would take 'rows' with it:\n{}",
        output
    );
}

/// When nothing the selection brings in is read, the line itself is the edit.
#[test]
fn test_a_selection_nothing_reads_still_says_remove_the_line() {
    let output = check_project(&[
        (
            "main.mi",
            concat!(
                "use local.lib.{rows, WordCount}\n",
                "\n",
                "fn main()\n",
                "    println(\"done\")\n",
            ),
        ),
        ("lib.mi", LIB),
    ]);

    assert!(
        output.contains("MER_IMP_005"),
        "neither name is read:\n{}",
        output
    );
    assert!(
        output.contains("remove the line"),
        "with nothing on it read, the line is what to delete:\n{}",
        output
    );
}

/// A type the file needs and has not imported is reported where the file needs
/// it. The type expression the compiler builds from the callee's declaration
/// carries no source range of its own, and a report left to speak for one lands
/// on the first byte of the file.
#[test]
fn test_a_type_missing_from_a_signature_reports_at_the_use_site() {
    let output = check_project_report(&[
        (
            "main.mi",
            concat!(
                "use system.io\n",
                "use local.lib.{rows}\n",
                "\n",
                "fn main()\n",
                "    let r = rows()\n",
                "    let first = r[0]\n",
                "    println(f\"{first.count}\")\n",
            ),
        ),
        ("lib.mi", LIB),
    ]);

    assert!(
        output.contains("MER_TYP_043"),
        "'WordCount' is not in scope here:\n{}",
        output
    );
    assert!(
        output.contains("main.mi:6:"),
        "the report belongs on the line that reads an element, not on line 1:\n{}",
        output
    );
}

/// The report has just said this name does not resolve. Offering it back as the
/// name the author meant tells them to write what they already wrote.
#[test]
fn test_a_missing_type_is_never_suggested_as_its_own_correction() {
    let output = check_project_report(&[
        (
            "main.mi",
            concat!(
                "use system.io\n",
                "use local.lib.{rows}\n",
                "\n",
                "fn main()\n",
                "    let r = rows()\n",
                "    let first = r[0]\n",
                "    println(f\"{first.count}\")\n",
            ),
        ),
        ("lib.mi", LIB),
    ]);

    assert!(
        output.contains("MER_TYP_043"),
        "the report this is about must be raised for its help to be checked:\n{}",
        output
    );
    assert!(
        !output.contains("Did you mean 'WordCount'?"),
        "a suggestion equal to the rejected name is no correction:\n{}",
        output
    );
}

/// A method is a callee too: the type its declaration returns is in scope for
/// the reader only because the import put it there.
#[test]
fn test_a_type_reached_through_a_methods_return_is_not_unused() {
    assert_project_no_warning(
        &[
            (
                "main.mi",
                concat!(
                    "use local.lib.{Report, WordCount}\n",
                    "\n",
                    "fn main()\n",
                    "    let report = Report()\n",
                    "    let first = report.rows()[0]\n",
                    "    println(f\"{first.count}\")\n",
                ),
            ),
            ("lib.mi", LIB),
        ],
        "MER_IMP_005",
    );
}

/// A module whose functions' signatures name its own alias, trait and type set.
const SIGNATURES: &str = concat!(
    "public type Id is int\n",
    "public type Real is f32 or float\n",
    "\n",
    "public trait Named\n",
    "    fn name() String\n",
    "\n",
    "public class Q implements Named\n",
    "    fn name() String\n",
    "        return \"q\"\n",
    "\n",
    "public fn ident(i Id) Id\n",
    "    return i\n",
    "\n",
    "public fn named<T implements Named>(x T) String\n",
    "    return x.name()\n",
    "\n",
    "public fn mkq() Q\n",
    "    return Q()\n",
    "\n",
    "public fn twice(x Real) Real\n",
    "    return x * 2.0\n",
);

/// A parameter type is declared by the callee, as its return type is: a call
/// reads it whether or not the caller imported the alias it names.
#[test]
fn test_a_callees_parameter_type_resolves_without_being_imported() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.sig.{ident}\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{ident(4)}\")\n",
                ),
            ),
            ("sig.mi", SIGNATURES),
        ],
        "4",
    );
}

/// A bound is part of the callee's declaration too: a call checks it against
/// the callee's own trait, which the caller never names.
#[test]
fn test_a_callees_trait_bound_resolves_without_being_imported() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.sig.{named, mkq}\n",
                    "\n",
                    "fn main()\n",
                    "    println(named(mkq()))\n",
                ),
            ),
            ("sig.mi", SIGNATURES),
        ],
        "q",
    );
}

/// A type-set bound, written as the shorthand, is read the same way: the call
/// binds the callee's parameter to f32 without the caller importing `Real`.
#[test]
fn test_a_callees_type_set_resolves_without_being_imported() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.sig.{twice}\n",
                    "\n",
                    "fn main()\n",
                    "    let a f32 = 1.5\n",
                    "    let r f32 = twice(a)\n",
                    "    println(f\"{r}\")\n",
                ),
            ),
            ("sig.mi", SIGNATURES),
        ],
        "3.0",
    );
}

/// Lifting the gate for the callee's declaration leaves the caller's own
/// writing gated: naming the unimported alias is still refused.
#[test]
fn test_a_caller_still_cannot_write_a_type_it_did_not_import() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.sig.{ident}\n",
                    "\n",
                    "fn main()\n",
                    "    let i Id = ident(4)\n",
                ),
            ),
            ("sig.mi", SIGNATURES),
        ],
        "Unknown type: Id",
    );
}
