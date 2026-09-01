// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Proves that the example in each diagnostic's documentation really produces
//! that diagnostic.
//!
//! A documented example that does not reproduce its own error is worse than no
//! example: a reader copies it, sees something else, and stops trusting the
//! page. Every live code therefore sits in exactly one of two lists below —
//! either its example is checked here against the real compiler, or it carries
//! a written reason why no single source file can produce it. A newly added
//! code that appears in neither list fails the partition test, so the lists
//! cannot quietly fall behind the registry.

use miri::diagnostics::DiagnosticCode;
use std::path::PathBuf;
use std::process::Command;

/// Codes whose `Before` example is verified to emit that code.
const VERIFIED: &[&str] = &[
    "MER_IMP_002",
    "MER_LEX_001",
    "MER_LEX_002",
    "MER_LEX_003",
    "MER_LEX_005",
    "MER_LEX_006",
    "MER_LEX_007",
    "MER_LEX_008",
    "MER_LEX_011",
    "MER_LEX_012",
    "MER_NAM_001",
    "MER_NAM_002",
    "MER_OWN_001",
    "MER_OWN_003",
    "MER_OWN_004",
    "MER_PAR_001",
    "MER_PAR_002",
    "MER_PAR_003",
    "MER_PAR_004",
    "MER_PAR_005",
    "MER_PAR_010",
    "MER_PAR_011",
    "MER_PAR_012",
    "MER_PAR_013",
    "MER_PAR_014",
    "MER_PAR_015",
    "MER_PAR_016",
    "MER_PAR_017",
    "MER_PAR_018",
    "MER_PAR_019",
    "MER_PAR_020",
    "MER_PAR_021",
    "MER_TAR_002",
    "MER_TAR_005",
    "MER_TAR_006",
    "MER_TAR_007",
    "MER_TAR_008",
    "MER_TAR_009",
    "MER_TYP_002",
    "MER_TYP_011",
    "MER_TYP_012",
    "MER_TYP_013",
    "MER_TYP_014",
    "MER_TYP_015",
    "MER_TYP_016",
    "MER_TYP_017",
    "MER_TYP_018",
    "MER_TYP_023",
    "MER_TYP_024",
    "MER_TYP_025",
    "MER_TYP_026",
    "MER_TYP_027",
    "MER_TYP_030",
    "MER_TYP_031",
    "MER_TYP_032",
    "MER_TYP_033",
    "MER_TYP_034",
    "MER_TYP_039",
    "MER_TYP_040",
    "MER_TYP_041",
    "MER_TYP_042",
    "MER_TYP_043",
    "MER_TYP_044",
    "MER_TYP_048",
    "MER_TYP_051",
    "MER_TYP_053",
    "MER_TYP_054",
    "MER_TYP_060",
    "MER_TYP_063",
    "MER_TYP_065",
    "MER_TYP_067",
    "MER_TYP_068",
];

/// Codes whose example cannot be verified this way, each with the reason.
const NOT_VERIFIABLE: &[(&str, &str)] = &[
    ("MER_BLD_001", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_BLD_002", "emitted by CLI refusal logic, not by compiling a source file"),
    ("MER_BLD_003", "emitted by comparing two builds' artifacts, not by compiling a source file"),
    ("MER_BLD_004", "emitted by the view command's name resolution, not by compiling a source file"),
    ("MER_BLD_005", "emitted by the view command's name resolution, not by compiling a source file"),
    ("MER_BLD_006", "emitted by the view or patch command's anchor matching, not by compiling a source file"),
    ("MER_BLD_007", "emitted by the view or patch command's anchor matching, not by compiling a source file"),
    ("MER_BLD_008", "reports a file the command could not read, not a property of any source file"),
    ("MER_BLD_009", "emitted by the patch command when given a stale SHA-256, not by compiling a source file"),
    ("MER_BLD_010", "emitted by the patch command's token alignment, not by compiling a source file"),
    ("MER_BLD_011", "emitted by the patch command when re-validation fails, not by compiling a source file"),
    ("MER_BLD_012", "reports edit flags that do not describe a coherent edit, not a property of any source file"),
    ("MER_BLD_013", "names a skill the build does not carry, which is a property of the binary rather than of any source file"),
    ("MER_BLD_014", "reports an installed skill file that has been edited since it was written, not a property of any source file"),
    ("MER_BLD_015", "reports a directory the skill command cannot write to, not a property of any source file"),
    ("MER_BLD_016", "reports a skill embedded in this build whose header cannot be read, not a property of any source file"),
    ("MER_BLD_017", "emitted by the patch command's insert operation when a declaration already exists, not by compiling a source file"),
    ("MER_BLD_018", "emitted by the fmt command when a second render disagrees with the first, not by compiling a source file"),
    ("MER_BLD_019", "emitted by the fmt command when rendering would drop a comment, not by compiling a source file"),
    ("MER_CG_001", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_CG_002", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_CG_003", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_CG_004", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_CG_005", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_CG_006", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_CG_007", "selected by a build flag rather than by source"),
    ("MER_CG_008", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_IMP_001", "needs several files; there is no single-file reproduction"),
    ("MER_IMP_003", "needs several files; there is no single-file reproduction"),
    ("MER_LEX_009", "no check currently reports this condition"),
    ("MER_LEX_010", "shadowed: MER_LEX_001 is reported first"),
    ("MER_MIR_001", "shadowed: MER_PAR_001 is reported first"),
    ("MER_MIR_002", "shadowed: MER_PAR_001 is reported first"),
    ("MER_MIR_003", "shadowed: MER_TYP_034 is reported first"),
    ("MER_MIR_004", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_MIR_005", "shadowed: MER_TAR_005 is reported first"),
    ("MER_MIR_006", "shadowed: MER_TYP_050 is reported first"),
    ("MER_MIR_007", "shadowed: MER_PAR_004 is reported first"),
    ("MER_MIR_008", "shadowed: MER_PAR_001 is reported first"),
    ("MER_MIR_009", "no check currently reports this condition"),
    ("MER_MIR_010", "shadowed: MER_TYP_030 is reported first"),
    ("MER_MIR_011", "shadowed: MER_TYP_043 is reported first"),
    ("MER_MIR_012", "shadowed: MER_TYP_030 is reported first"),
    ("MER_MIR_013", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_MIR_014", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_NAM_003", "shadowed: MER_PAR_001 is reported first"),
    ("MER_OWN_002", "shadowed: MER_PAR_001 is reported first"),
    ("MER_PAR_006", "shadowed: MER_PAR_001 is reported first"),
    ("MER_PAR_007", "shadowed: MER_LEX_001 is reported first"),
    ("MER_PAR_008", "shadowed: MER_TYP_034 is reported first"),
    ("MER_PAR_009", "shadowed: MER_PAR_001 is reported first"),
    ("MER_RT_001", "traps while the program runs, which a static check cannot observe"),
    ("MER_RT_002", "traps while the program runs, which a static check cannot observe"),
    ("MER_RT_003", "shadowed: MER_TYP_034 is reported first"),
    ("MER_RT_004", "shadowed: MER_TYP_034 is reported first"),
    ("MER_RT_005", "traps while the program runs, which a static check cannot observe"),
    ("MER_TAR_001", "shadowed: MER_TYP_034 is reported first"),
    ("MER_TAR_003", "needs GPU lowering, which a host check does not perform"),
    ("MER_TAR_004", "shadowed: MER_TYP_034 is reported first"),
    ("MER_TYP_019", "no check currently reports this condition"),
    ("MER_TYP_020", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_021", "no check currently reports this condition"),
    ("MER_TYP_022", "no check currently reports this condition"),
    ("MER_TYP_029", "shadowed: MER_TYP_033 is reported first"),
    ("MER_TYP_035", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_036", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_037", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_038", "shadowed: MER_PAR_014 is reported first"),
    ("MER_TYP_047", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_049", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_050", "shadowed: MER_TAR_005 is reported first"),
    ("MER_TYP_052", "shadowed: MER_TYP_034 is reported first"),
    ("MER_TYP_055", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_056", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_057", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_058", "shadowed: MER_PAR_013 is reported first"),
    ("MER_TYP_059", "shadowed: MER_PAR_014 is reported first"),
    ("MER_TYP_062", "shadowed: MER_PAR_001 is reported first"),
    ("MER_TYP_064", "no check currently reports this condition"),
    ("MER_TYP_066", "shadowed: MER_PAR_001 is reported first"),
];

/// Read the `Before` example out of a code's documentation.
fn before_example(code: DiagnosticCode) -> Option<String> {
    let doc = code.doc();
    let start = doc.find("## Before")? + "## Before".len();
    let rest = &doc[start..];
    let end = rest.find("## ").map(|p| start + p).unwrap_or(doc.len());
    let section = &doc[start..end];
    let open = section.find("```miri")? + "```miri".len();
    let body = &section[open..];
    let close = body.find("```")?;
    Some(body[..close].trim_start_matches('\n').to_string())
}

/// Diagnostic codes reported by `miri check` for one source file.
///
/// The file is written inside the repository because module resolution is
/// relative to the working directory: the same source checked from elsewhere
/// fails to find the standard library and reports that instead of the error
/// under test.
fn codes_reported_for(source: &str, slot: &str) -> Vec<String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/explain-snippets");
    std::fs::create_dir_all(&dir).expect("could not create the snippet directory");
    let path = dir.join(format!("{}.mi", slot));
    std::fs::write(&path, source).expect("could not write the snippet");

    let binary = PathBuf::from(env!("CARGO_BIN_EXE_miri"));
    let output = Command::new(binary)
        .args(["check", &path.display().to_string(), "--format", "json"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("could not run the compiler");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&stdout) else {
        return vec![];
    };
    envelope["diagnostics"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|d| d["code"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn every_live_code_is_either_verified_or_carries_a_reason() {
    for code in DiagnosticCode::all() {
        if code.is_reserved() {
            continue;
        }
        let wire = code.as_str();
        let verified = VERIFIED.contains(&wire);
        let excused = NOT_VERIFIABLE.iter().any(|(c, _)| *c == wire);
        assert!(
            verified != excused,
            "{} must be in exactly one of the two lists: its example is either \
             checked against the compiler, or it states why it cannot be",
            wire
        );
    }
}

#[test]
fn verified_examples_emit_the_code_they_document() {
    for wire in VERIFIED {
        let code: DiagnosticCode = wire
            .parse()
            .unwrap_or_else(|_| panic!("{} is not a registered code", wire));
        let example =
            before_example(code).unwrap_or_else(|| panic!("{} has no Before example", wire));
        let reported = codes_reported_for(&example, wire);
        assert_eq!(
            reported.first().map(String::as_str),
            Some(*wire),
            "the documented example for {} must report it first; it reported {:?}",
            wire,
            reported
        );
    }
}

#[test]
fn every_reason_is_written_out() {
    for (wire, reason) in NOT_VERIFIABLE {
        assert!(
            reason.len() > 20,
            "{} is excused without a real reason",
            wire
        );
    }
}

/// An excuse has to stay true, or the list becomes a place to hide failures.
///
/// Every excused code whose documentation still carries a runnable example is
/// run here, and the example must NOT report the code it illustrates — because
/// that is precisely the claim the excuse makes. A code that starts reporting
/// itself has become verifiable and belongs on the other list; this fails until
/// it is moved, so the excuse cannot quietly outlive the reason for it.
#[test]
fn excused_examples_still_fail_to_report_their_code() {
    for (wire, _) in NOT_VERIFIABLE {
        let Ok(code) = wire.parse::<DiagnosticCode>() else {
            panic!("{} is not a registered code", wire);
        };
        // Some excused codes are documented with prose rather than source, and
        // a few need several files; neither has a single example to run.
        let Some(example) = before_example(code) else {
            continue;
        };
        if example.contains("// file:") {
            continue;
        }
        let reported = codes_reported_for(&example, wire);
        assert!(
            !reported.iter().any(|c| c == wire),
            "{} is on the excused list, but its example now reports it ({:?}); \
             move it to the verified list",
            wire,
            reported
        );
    }
}
