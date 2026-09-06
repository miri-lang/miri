// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The listing `explain --list` publishes says which codes a repair can be
//! reached from. This drives a real program for every pair it claims, so the
//! claim is a measurement rather than an intention.
//!
//! A repair whose emitting site moves to a different code, or is deleted, fails
//! here rather than quietly leaving a tool waiting for an edit that never comes.

use crate::utils::miri_cmd;
use miri::diagnostics::json::DiagnosticsEnvelope;
use miri::diagnostics::repair::RepairId;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// A program that provokes one repair, and the code it is expected to arrive on.
struct Provocation {
    repair: RepairId,
    code: &'static str,
    source: &'static str,
}

/// One program per pair the registry claims, keyed by the pair it proves.
///
/// A repair reachable from more than one code needs one entry per code: the
/// pair is what the listing publishes, so the pair is what is measured.
const PROVOCATIONS: &[Provocation] = &[
    Provocation {
        repair: RepairId::LetToVar,
        code: "MER_TYP_042",
        source: "fn main()\n    let x = 1\n    x = 2\n",
    },
    Provocation {
        repair: RepairId::AddImport,
        code: "MER_TYP_034",
        source: "fn main()\n    let x = sqrt(4.0)\n    println(f\"{x}\")\n",
    },
    Provocation {
        repair: RepairId::AddImport,
        code: "MER_TYP_043",
        source: "fn main()\n    let m = Map({\"a\": \"b\"})\n    println(f\"{m}\")\n",
    },
    Provocation {
        repair: RepairId::DropExtraArguments,
        code: "MER_TYP_002",
        source: "fn add(a int) int\n    a\n\nfn main()\n    println(f\"{add(1, 2)}\")\n",
    },
    Provocation {
        repair: RepairId::ColonAnnotation,
        code: "MER_PAR_001",
        source: "fn main()\n    let x: int = 5\n    println(f\"{x}\")\n",
    },
    Provocation {
        repair: RepairId::ArrowReturnType,
        code: "MER_PAR_001",
        source: "fn f() -> int\n    1\n\nfn main()\n    println(f\"{f()}\")\n",
    },
    Provocation {
        repair: RepairId::LetMutToVar,
        code: "MER_PAR_001",
        source: "fn main()\n    let mut x = 1\n    x = 2\n    println(f\"{x}\")\n",
    },
    Provocation {
        repair: RepairId::NullToNone,
        code: "MER_TYP_034",
        source: "fn main()\n    let x int? = null\n    println(f\"{x}\")\n",
    },
    Provocation {
        repair: RepairId::PrintlnBang,
        code: "MER_LEX_001",
        source: "fn main()\n    println!(\"hi\")\n",
    },
    Provocation {
        repair: RepairId::DropIteratorAccessor,
        code: "MER_TYP_033",
        source: "use system.collections.map.{Map}\n\nfn main()\n    let tags = \
                 Map({\"a\": \"b\"})\n    for k in tags.keys()\n        println(k)\n",
    },
    Provocation {
        repair: RepairId::ConcatToFormattedString,
        code: "MER_TYP_002",
        source: "fn main()\n    let qty = 3\n    let s = \"x\" + \" \" + qty\n    println(s)\n",
    },
    Provocation {
        repair: RepairId::QualifyVariantPattern,
        code: "MER_TYP_038",
        source: "fn get() Result<int, String>\n    Result.Ok(1)\n\nfn main()\n    match \
                 get()\n        Ok(n): println(f\"{n}\")\n        Result.Err(e): println(e)\n",
    },
];

/// The (code, repair) pairs `miri fix` reports for `source`.
fn pairs_reported_for(name: &str, source: &str) -> BTreeSet<(String, String)> {
    let directory: PathBuf = std::env::temp_dir().join(format!("miri-repair-reach-{}", name));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("could not create the fixture directory");
    let file = directory.join("main.mi");
    fs::write(&file, source).expect("could not write the fixture source");

    let output = miri_cmd()
        .arg("fix")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .output()
        .expect("failed to run the fix command");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = fs::remove_dir_all(&directory);

    let envelope: DiagnosticsEnvelope =
        serde_json::from_str(&stdout).expect("fix did not emit a parseable envelope");
    envelope
        .diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let code = diagnostic.code.clone()?;
            let repair = diagnostic.repair.as_ref()?;
            Some((code, repair.id.clone()))
        })
        .collect()
}

#[test]
fn test_every_published_pair_is_produced_by_a_program() {
    for (index, provocation) in PROVOCATIONS.iter().enumerate() {
        let reported = pairs_reported_for(&format!("pair-{}", index), provocation.source);
        let expected = (
            provocation.code.to_string(),
            provocation.repair.as_str().to_string(),
        );
        assert!(
            reported.contains(&expected),
            "{} should reach {}; the run reported {:?}",
            provocation.repair.as_str(),
            provocation.code,
            reported
        );
    }
}

#[test]
fn test_the_registry_claims_exactly_the_pairs_that_are_proven() {
    let claimed: BTreeSet<(String, String)> = RepairId::all()
        .iter()
        .flat_map(|repair| {
            repair
                .codes()
                .iter()
                .map(move |code| (code.as_str().to_string(), repair.as_str().to_string()))
        })
        .collect();
    let proven: BTreeSet<(String, String)> = PROVOCATIONS
        .iter()
        .map(|provocation| {
            (
                provocation.code.to_string(),
                provocation.repair.as_str().to_string(),
            )
        })
        .collect();

    assert_eq!(
        claimed, proven,
        "every pair the listing publishes needs a program above that produces it, and \
         every program above needs the pair it produces to be published"
    );
}

#[test]
fn test_a_code_with_no_repair_publishes_an_empty_list() {
    let repaired: BTreeSet<&str> = RepairId::all()
        .iter()
        .flat_map(|repair| repair.codes().iter().map(|code| code.as_str()))
        .collect();

    let unrepaired = miri::diagnostics::DiagnosticCode::all()
        .iter()
        .find(|code| !repaired.contains(code.as_str()))
        .expect("some code carries no repair");

    assert!(
        miri::diagnostics::repair::repairs_for(*unrepaired).is_empty(),
        "{} carries no repair, so its published list should be empty",
        unrepaired.as_str()
    );
}
