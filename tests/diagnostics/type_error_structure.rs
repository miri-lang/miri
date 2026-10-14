// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every type error that compares two types has one shape: the message names
//! what was found and what was expected, both spelled as source spells them,
//! and the help says how to fix it. The diagnostic carries the two types as
//! structured fields too, so a tool reads them without parsing the sentence.
//!
//! Two gates. The first drives a program for every place a type is compared and
//! reads the envelope `check` reports. The second reads the compiler source and
//! requires every comparison code to be raised through the one constructor that
//! cannot be called without both types and a help, so a new site cannot be
//! written ad hoc.

use crate::utils::miri_cmd;
use miri::diagnostics::json::{DiagnosticsEnvelope, JsonDiagnostic};
use std::fs;
use std::path::Path;

/// The codes that report a value of one type where another belongs.
const TYPE_COMPARISON_CODES: &[&str] = &[
    "TypTypeMismatch",
    "TypIncompatibleTypesInOperation",
    "TypCollectionElementType",
    "TypRangeTypeMismatch",
    "TypInvalidCast",
    "TypAtomicBufferTypeMismatch",
];

/// The constructors a comparison code may be raised through.
const MISMATCH_CONSTRUCTORS: &[&str] = &[
    "report_type_mismatch(",
    "report_type_mismatch_at_site(",
    "push_type_mismatch(",
];

/// Places a comparison code is spelled without being raised, or raised outside
/// the type checker, each with why.
const SPELLED_ELSEWHERE: &[(&str, &str)] = &[
    (
        "src/diagnostics/",
        "the registry declares the codes and the repair table names them",
    ),
    (
        "src/mir/lowering/expression/call_expr.rs",
        "lowering refuses an atomic buffer only an instantiation reveals, with the words \
         `atomic_buffer_mismatch` builds for the type checker",
    ),
];

/// A program that compares two types the way one construct does.
struct Comparison {
    what: &'static str,
    code: &'static str,
    source: &'static str,
    expected: &'static str,
    found: &'static str,
}

const COMPARISONS: &[Comparison] = &[
    Comparison {
        what: "a declared type given a value of another",
        code: "MER_TYP_002",
        source: "fn main()\n    let label String = 5\n    println(label)\n",
        expected: "String",
        found: "int",
    },
    Comparison {
        what: "a variable assigned a value of another type",
        code: "MER_TYP_002",
        source:
            "fn main()\n    var total i32 = 1\n    total = \"many\"\n    println(f\"{total}\")\n",
        expected: "i32",
        found: "String",
    },
    Comparison {
        what: "an argument of the wrong type",
        code: "MER_TYP_002",
        source: "fn twice(x int) int\n    x * 2\n\nfn main()\n    let r = twice(\"a\")\n    \
                 println(f\"{r}\")\n",
        expected: "int",
        found: "String",
    },
    Comparison {
        what: "a returned value of the wrong type",
        code: "MER_TYP_002",
        source: "fn name() int\n    return \"a\"\n\nfn main()\n    println(f\"{name()}\")\n",
        expected: "int",
        found: "String",
    },
    Comparison {
        what: "a condition that is not a boolean",
        code: "MER_TYP_002",
        source: "fn main()\n    let n = 1\n    if n\n        println(\"x\")\n",
        expected: "bool",
        found: "int",
    },
    Comparison {
        what: "an operator applied to two types that do not combine",
        code: "MER_TYP_002",
        source: "fn main()\n    let x = 1 + \"a\"\n    println(f\"{x}\")\n",
        expected: "int",
        found: "String",
    },
    Comparison {
        what: "a list element of another type",
        code: "MER_TYP_002",
        source:
            "use system.collections.list\n\nfn main()\n    let xs List<int> = List([1, 2])\n    \
                 xs.push(\"a\")\n",
        expected: "int",
        found: "String",
    },
    Comparison {
        what: "a collection literal mixing element types",
        code: "MER_TYP_039",
        source: "fn main()\n    let xs = [1, \"a\"]\n    println(f\"{xs.length()}\")\n",
        expected: "int",
        found: "String",
    },
    Comparison {
        what: "a map read with a key of another type",
        code: "MER_TYP_039",
        source: "use system.collections.map\n\nfn main()\n    let m = {\"a\": 1}\n    \
                 println(f\"{m[1]}\")\n",
        expected: "String",
        found: "int",
    },
    Comparison {
        what: "a range whose ends differ in type",
        code: "MER_TYP_051",
        source: "fn main()\n    for i in 0..2.5\n        println(f\"{i}\")\n",
        expected: "int",
        found: "float",
    },
    Comparison {
        what: "a cast from a type that has no numeric value",
        code: "MER_TYP_063",
        source: "fn main()\n    let n = \"5\" as int\n    println(f\"{n}\")\n",
        expected: "a numeric type",
        found: "String",
    },
    Comparison {
        what: "an atomic operation on a buffer that holds no atomics",
        code: "MER_TYP_029",
        source: "use system.collections.array\nuse system.gpu.atomic\n\nfn main()\n    \
                 gpu var hist = Array<f32, 4>()\n    gpu forall i in 0..4\n        \
                 atomic_add(hist, i, 1.0)\n",
        expected: "Array<Atomic<i32>, N> or Array<Atomic<u32>, N>",
        found: "Array<f32, 4>",
    },
];

/// The envelope `miri check` reports for `source`, written under `name`.
fn checked(name: &str, source: &str) -> DiagnosticsEnvelope {
    let directory = std::env::temp_dir().join(format!("miri-type-structure-{name}"));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("could not create the fixture directory");
    let file = directory.join("main.mi");
    fs::write(&file, source).expect("could not write the fixture source");
    let output = miri_cmd()
        .arg("check")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .output()
        .expect("failed to run the check command");
    let _ = fs::remove_dir_all(&directory);
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("check did not emit a parseable envelope:\n{stdout}"))
}

/// What is missing from `diagnostic` for it to have the shape of a type error.
fn shape_gaps(diagnostic: &JsonDiagnostic, comparison: &Comparison) -> Vec<String> {
    let message = diagnostic.message.as_str();
    let mut gaps = Vec::new();
    if !message.contains(comparison.found) {
        gaps.push(format!(
            "the message does not name the found type `{}`",
            comparison.found
        ));
    }
    // An expected side that describes what would do (`a numeric type`) rather
    // than naming one type is the message's own wording; one naming types
    // names each of them.
    let describes = comparison.expected.starts_with("a ");
    for alternative in comparison.expected.split(" or ") {
        if !describes && !message.contains(alternative) {
            gaps.push(format!(
                "the message does not name the expected type `{alternative}`"
            ));
        }
    }
    if diagnostic.expected.as_deref() != Some(comparison.expected) {
        gaps.push(format!(
            "the expected field is {:?}, not {:?}",
            diagnostic.expected, comparison.expected
        ));
    }
    if diagnostic.actual.as_deref() != Some(comparison.found) {
        gaps.push(format!(
            "the actual field is {:?}, not {:?}",
            diagnostic.actual, comparison.found
        ));
    }
    if diagnostic.help.as_deref().is_none_or(str::is_empty) {
        gaps.push("there is no help saying how to fix it".to_string());
    }
    gaps
}

#[test]
fn test_every_type_comparison_names_both_types_and_the_fix() {
    let mut failures = Vec::new();
    for (index, comparison) in COMPARISONS.iter().enumerate() {
        let envelope = checked(&index.to_string(), comparison.source);
        let Some(diagnostic) = envelope
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_deref() == Some(comparison.code))
        else {
            failures.push(format!(
                "{} should be refused under {}; check reported {:?}",
                comparison.what,
                comparison.code,
                envelope
                    .diagnostics
                    .iter()
                    .map(|d| (d.code.clone(), d.message.clone()))
                    .collect::<Vec<_>>()
            ));
            continue;
        };
        for gap in shape_gaps(diagnostic, comparison) {
            failures.push(format!(
                "{}: {} ({:?})",
                comparison.what, gap, diagnostic.message
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every `.rs` file under `src/`, as (path relative to the crate, text without
/// comment lines).
fn compiler_sources() -> Vec<(String, String)> {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<(String, String)>) {
        let entries = fs::read_dir(directory).expect("could not read the compiler sources");
        for entry in entries {
            let path = entry.expect("could not read a source entry").path();
            if path.is_dir() {
                walk(root, &path, files);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let text = fs::read_to_string(&path).expect("could not read a source file");
                let code: Vec<&str> = text
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect();
                let relative = path
                    .strip_prefix(root)
                    .expect("a source file lies under the crate")
                    .to_string_lossy()
                    .into_owned();
                files.push((relative, code.join("\n")));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(root, &root.join("src"), &mut files);
    files
}

/// Whether the text before a code is the opening of a mismatch constructor's
/// call, so the code is the argument that constructor raises.
fn raised_through_a_mismatch_constructor(before: &str) -> bool {
    let before = before.trim_end();
    MISMATCH_CONSTRUCTORS
        .iter()
        .any(|constructor| before.ends_with(constructor))
}

#[test]
fn test_type_comparison_codes_are_raised_through_the_mismatch_constructor() {
    let mut stray = Vec::new();
    for (path, text) in compiler_sources() {
        if SPELLED_ELSEWHERE
            .iter()
            .any(|(place, _)| path.starts_with(place))
        {
            continue;
        }
        for code in TYPE_COMPARISON_CODES {
            let token = format!("DiagnosticCode::{code}");
            for (at, _) in text.match_indices(&token) {
                let follows = text[at + token.len()..].chars().next();
                if follows.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                if !raised_through_a_mismatch_constructor(&text[..at]) {
                    let line = text[..at].lines().count();
                    stray.push(format!(
                        "{path}:{line} raises {code} outside the constructor"
                    ));
                }
            }
        }
    }
    assert!(
        stray.is_empty(),
        "type comparison codes must be raised through report_type_mismatch, which takes \
         both types and a help:\n{}",
        stray.join("\n")
    );
}
