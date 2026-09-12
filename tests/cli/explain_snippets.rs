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
    "MER_IMP_004",
    "MER_IMP_005",
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
    "MER_TYP_038",
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
    "MER_TYP_069",
    "MER_TYP_070",
    "MER_TYP_071",
    "MER_TYP_072",
    "MER_TYP_073",
    "MER_TYP_074",
    "MER_TYP_075",
];

/// Codes whose example cannot be verified this way, each with the reason.
const NOT_VERIFIABLE: &[(&str, &str)] = &[
    ("MER_BLD_021", "raised by run/build about the program as a whole; documented with shell prose because no source line carries it"),
    ("MER_BLD_022", "raised by the view command when asked about a type name not in scope, not by compiling a source file"),
    ("MER_BLD_023", "raised by the view command when an anchor narrows nothing, not by compiling a source file"),
    ("MER_BLD_024", "raised by the agent session when a message on stdin is not a Content-Length frame, not by compiling a source file"),
    ("MER_BLD_025", "raised by the test runner when a walk turned up no `@test` at all, not by compiling a source file"),
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
    ("MER_BLD_020", "reports that an apply wrote nothing because no error carried a repair, not a property of any source file"),
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
    ("MER_RT_006", "kills the program with a signal at run time, which a static check cannot observe"),
    ("MER_RT_007", "kills the program with a signal at run time, which a static check cannot observe"),
    ("MER_RT_008", "kills the program with a signal at run time, which a static check cannot observe"),
    ("MER_RT_009", "kills the program with a signal at run time, which a static check cannot observe"),
    ("MER_RT_010", "kills the program with a signal at run time, which a static check cannot observe"),
    ("MER_RT_011", "traps while the program runs, which a static check cannot observe"),
    ("MER_RT_012", "traps while the program runs, which a static check cannot observe"),
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

/// What one documented example reports when the compiler really reads it.
struct Reported {
    message: String,
    help: Option<String>,
}

/// Compile a source file and extract the first diagnostic it reports.
///
/// The file is written inside the repository because module resolution is
/// relative to the working directory: the same source checked from elsewhere
/// fails to find the standard library and reports that instead of the error
/// under test.
fn first_diagnostic(source: &str, slot: &str) -> Option<Reported> {
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
        return None;
    };
    envelope["diagnostics"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|d| {
            Some(Reported {
                message: d["message"].as_str()?.to_string(),
                help: d["help"].as_str().map(str::to_string),
            })
        })
}

/// All diagnostic codes reported by `miri check` for one source file.
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

/// Check if a message matches one of the documented shapes.
///
/// A shape is a template where `{identifier}` matches any sequence of
/// characters, including newlines. All other text is literal and must match
/// exactly.
///
/// A brace is a placeholder only when it wraps a bare identifier, because that
/// is the only thing the compiler ever substitutes: a `format!` writes
/// `{name}`, never `{' that opened this}`. Reading every brace as a placeholder
/// made a shape that merely *mentions* a brace match almost any message, which
/// is how a page could declare a rule and gate nothing.
fn message_matches_shape(message: &str, shape: &str) -> bool {
    let chars: Vec<char> = shape.chars().collect();
    let mut regex_str = String::from("^(?s)");
    let mut index = 0;

    while index < chars.len() {
        if let Some(after) = placeholder_end(&chars, index) {
            regex_str.push_str("([\\s\\S]+)");
            index = after;
            continue;
        }
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'n') {
            regex_str.push('\n');
            index += 2;
            continue;
        }
        push_literal(&mut regex_str, chars[index]);
        index += 1;
    }
    regex_str.push('$');

    match regex::Regex::new(&regex_str) {
        Ok(re) => re.is_match(message),
        Err(e) => {
            panic!(
                "shape '{}' produced an invalid regex: {} — fix the shape definition",
                shape, e
            );
        }
    }
}

/// The index just past `{identifier}` starting at `open`, or None if the brace
/// at `open` is literal text rather than a substitution point.
fn placeholder_end(chars: &[char], open: usize) -> Option<usize> {
    if chars.get(open) != Some(&'{') {
        return None;
    }
    let first = *chars.get(open + 1)?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let mut cursor = open + 2;
    while let Some(ch) = chars.get(cursor) {
        if *ch == '}' {
            return Some(cursor + 1);
        }
        if !ch.is_ascii_alphanumeric() && *ch != '_' {
            return None;
        }
        cursor += 1;
    }
    None
}

/// Append one shape character to the regex, escaping what regex syntax claims.
fn push_literal(regex_str: &mut String, ch: char) {
    match ch {
        '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' => {
            regex_str.push('\\');
            regex_str.push(ch);
        }
        _ => regex_str.push(ch),
    }
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
    let mut failures = Vec::new();

    for wire in VERIFIED {
        let code: DiagnosticCode = match wire.parse() {
            Ok(c) => c,
            Err(_) => {
                failures.push(format!("{} is not a registered code", wire));
                continue;
            }
        };

        let example = match before_example(code) {
            Some(e) => e,
            None => {
                failures.push(format!("{} has no Before example", wire));
                continue;
            }
        };

        let reported = codes_reported_for(&example, wire);
        if reported.first().map(String::as_str) != Some(*wire) {
            failures.push(format!(
                "{}: the documented example must report it first; it reported {:?}",
                wire, reported
            ));
        }

        // Also verify that the message matches a documented shape
        let explanation = code.explanation();
        if explanation.messages.is_empty() {
            failures.push(format!(
                "{} has no ## Messages section documenting the shapes this code can emit. \
                 Add a ## Messages section with backticked message shapes.",
                wire
            ));
            continue;
        }

        match first_diagnostic(&example, wire) {
            Some(reported) => {
                let matches_any = explanation
                    .messages
                    .iter()
                    .any(|shape| message_matches_shape(&reported.message, shape));

                if !matches_any {
                    failures.push(format!(
                        "{}: the first diagnostic message '{}' does not match any declared shape: {:?}",
                        wire, reported.message, explanation.messages
                    ));
                }
            }
            None => {
                failures.push(format!("{}: could not extract the first diagnostic", wire));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} verified codes failed:\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }
}

/// Codes whose documented example reports a diagnostic carrying no `help`.
///
/// The `help` is the compiler's own one-line statement of the rule, and the
/// gate below makes the page carry it verbatim. A code that states no rule has
/// nothing to pin its page against, so its prose is unpoliced — listing them
/// here keeps that gap countable and stops it growing silently.
const NO_HELP_EMITTED: &[&str] = &[
    "MER_IMP_002",
    "MER_OWN_003",
    "MER_OWN_004",
    "MER_PAR_001",
    "MER_TAR_002",
    "MER_TAR_005",
    "MER_TAR_006",
    "MER_TAR_007",
    "MER_TAR_008",
    "MER_TAR_009",
    "MER_TYP_016",
    "MER_TYP_017",
    "MER_TYP_018",
    "MER_TYP_027",
    "MER_TYP_030",
    "MER_TYP_031",
    "MER_TYP_032",
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

/// A page must state the rule the compiler states.
///
/// `MER_LEX_012` is why this exists: its page told a reader to count braces
/// while the compiler reported a nested quote, and every gate passed, because
/// nothing compared the prose against what fires. Declaring the `help` on the
/// page ties the two together — the page cannot describe a rule the compiler
/// does not state, and a help reworded in the compiler reddens the page that
/// quotes it.
#[test]
fn verified_pages_declare_the_help_that_fires() {
    let mut failures = Vec::new();

    for wire in VERIFIED {
        let Ok(code) = wire.parse::<DiagnosticCode>() else {
            failures.push(format!("{} is not a registered code", wire));
            continue;
        };
        let Some(example) = before_example(code) else {
            failures.push(format!("{} has no Before example", wire));
            continue;
        };
        let Some(reported) = first_diagnostic(&example, wire) else {
            failures.push(format!("{}: could not extract the first diagnostic", wire));
            continue;
        };
        let declared = code.explanation().helps;
        let silent = NO_HELP_EMITTED.contains(wire);

        match reported.help {
            Some(help) => {
                if silent {
                    failures.push(format!(
                        "{} is listed as emitting no help, but it emitted '{}'; \
                         remove it from NO_HELP_EMITTED and declare the help on its page",
                        wire, help
                    ));
                    continue;
                }
                if declared.is_empty() {
                    failures.push(format!(
                        "{} emits the help '{}' but its page declares none. \
                         Add a ## Help section carrying it as a backticked shape.",
                        wire, help
                    ));
                    continue;
                }
                if !declared
                    .iter()
                    .any(|shape| message_matches_shape(&help, shape))
                {
                    failures.push(format!(
                        "{}: the help that fires, '{}', matches no shape its page declares: {:?}",
                        wire, help, declared
                    ));
                }
            }
            None => {
                if !silent {
                    failures.push(format!(
                        "{} emits no help; add it to NO_HELP_EMITTED, or give the \
                         diagnostic a help line that states the rule",
                        wire
                    ));
                }
                if !declared.is_empty() {
                    failures.push(format!(
                        "{} declares a ## Help section but its example emits no help, \
                         so nothing proves the declaration: {:?}",
                        wire, declared
                    ));
                }
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} pages disagree with the help that fires:\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }
}

/// A code listed as silent must be a live code whose page is verified.
#[test]
fn every_silent_code_is_verified() {
    for wire in NO_HELP_EMITTED {
        assert!(
            VERIFIED.contains(wire),
            "{} is listed as emitting no help, but its example is not verified, \
             so nothing ever runs it to find out",
            wire
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

/// Verify that every declared message shape actually appears in the compiler source.
///
/// A message shape with no literal text that is verifiable (at least 8 characters)
/// is considered too vague to gate. A shape that declares something the compiler
/// never emits is a documentation error — an invented message that may never show up
/// when a user encounters the code, breaking trust.
#[test]
fn declared_shapes_appear_in_compiler_sources() {
    use std::fs;
    use std::path::Path;

    // Read all Rust source files from src/ into a single buffer for fast searching.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut all_source = String::new();
    fn walk_source(dir: &Path, buffer: &mut String) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // Skip target directory
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |n| n == "target")
            {
                continue;
            }

            if path.is_dir() {
                walk_source(&path, buffer)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let content = fs::read_to_string(&path)?;
                buffer.push_str(&content);
                buffer.push('\n');
            }
        }
        Ok(())
    }

    walk_source(&src_dir, &mut all_source).expect("could not read src directory");

    // Normalize the source to handle Rust string literal line continuations:
    // a backslash immediately followed by a newline and any following whitespace
    // becomes nothing (what the Rust lexer does).
    all_source = normalize_rust_string_literals(&all_source);

    let mut failures = Vec::new();

    for wire in VERIFIED {
        let Ok(code) = wire.parse::<DiagnosticCode>() else {
            continue;
        };

        let explanation = code.explanation();
        let declared = explanation
            .messages
            .iter()
            .map(|shape| ("message", shape))
            .chain(explanation.helps.iter().map(|shape| ("help", shape)));

        for (kind, shape) in declared {
            let runs = literal_runs(shape);
            if runs.is_empty() {
                failures.push(format!(
                    "{}: {} shape '{}' is too vague to verify (no literal text ≥8 chars) — \
                     document the exact format string the compiler builds",
                    wire, kind, shape
                ));
                continue;
            }
            if !runs.iter().any(|run| source_carries(&all_source, run)) {
                failures.push(format!(
                    "{}: {} shape '{}' has no verifiable literal text in the compiler source",
                    wire, kind, shape
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} declared shapes do not appear in compiler source:\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }
}

/// The stretches of a shape that are literal text rather than substitution.
///
/// Only runs long enough to be distinctive are returned: a shape grounded on
/// `is` or `the` alone would match the compiler source by accident.
fn literal_runs(shape: &str) -> Vec<String> {
    const SHORTEST_DISTINCTIVE_RUN: usize = 8;
    let chars: Vec<char> = shape.chars().collect();
    let mut runs = Vec::new();
    let mut current = String::new();
    let mut index = 0;

    while index < chars.len() {
        if let Some(after) = placeholder_end(&chars, index) {
            if current.trim().chars().count() >= SHORTEST_DISTINCTIVE_RUN {
                runs.push(current.clone());
            }
            current.clear();
            index = after;
            continue;
        }
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'n') {
            current.push('\n');
            index += 2;
            continue;
        }
        current.push(chars[index]);
        index += 1;
    }
    if current.trim().chars().count() >= SHORTEST_DISTINCTIVE_RUN {
        runs.push(current);
    }
    runs
}

/// Whether the compiler source carries this run of a shape.
///
/// A shape records the text a reader sees, while the source spells it as a Rust
/// literal, so a quote reaches the page as `"` and the source as `\"`, and a
/// newline as an escape rather than as the character. Both spellings are tried
/// so a faithful shape is not rejected for the way Rust writes it down.
fn source_carries(source: &str, run: &str) -> bool {
    let escaped_quotes = run.replace('"', "\\\"");
    let escaped_newlines = run.replace('\n', "\\n");
    let both = escaped_quotes.replace('\n', "\\n");
    [
        run,
        escaped_quotes.as_str(),
        escaped_newlines.as_str(),
        both.as_str(),
    ]
    .iter()
    .any(|candidate| source.contains(candidate))
}

/// Normalize Rust source by collapsing string literal line continuations.
/// A backslash immediately followed by a newline and any following whitespace
/// becomes nothing (what the Rust lexer does with string literal continuations).
/// An escaped backslash (two backslashes followed by a newline) is NOT a continuation.
fn normalize_rust_string_literals(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'\n') {
            // This is a backslash followed by a newline. But we need to check if
            // the backslash itself is escaped (preceded by another backslash).
            // We've already added characters to result, so check if the last
            // character we added is a backslash.
            if result.ends_with('\\') {
                // The backslash we just processed is escaped by the previous backslash.
                // Add both the backslash and newline normally (the newline is NOT a continuation).
                result.push(ch);
            } else {
                // This is a genuine line continuation: backslash + newline + whitespace.
                // Consume the newline.
                chars.next();
                // Consume any following whitespace (spaces and tabs).
                while let Some(&c) = chars.peek() {
                    if c == ' ' || c == '\t' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                // Don't add anything to result; the continuation is removed.
            }
        } else {
            result.push(ch);
        }
    }

    result
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

#[test]
fn test_message_matches_shape_literal_text() {
    let shape = "Unknown type: {name}";
    assert!(message_matches_shape("Unknown type: MyClass", shape));
    // Placeholder matches any text including spaces, so "extra" is included
    assert!(message_matches_shape("Unknown type: MyClass extra", shape));
    // But a message that doesn't start with "Unknown type: " won't match
    assert!(!message_matches_shape("Type mismatch: MyClass", shape));
}

#[test]
fn test_message_matches_shape_regex_metacharacters_literal() {
    let shape = "Expected [a-z]+";
    assert!(message_matches_shape("Expected [a-z]+", shape));
    assert!(!message_matches_shape("Expected aaa", shape));
}

#[test]
fn test_message_matches_shape_multiple_placeholders() {
    let shape = "{lhs} {op} {rhs}";
    assert!(message_matches_shape("5 + 3", shape));
    assert!(message_matches_shape("hello world foo", shape));
    assert!(!message_matches_shape("5 +", shape));
}

#[test]
fn test_message_matches_shape_anchoring_no_prefix() {
    let shape = "Division by zero";
    assert!(message_matches_shape("Division by zero", shape));
    assert!(!message_matches_shape(
        "Division by zero in constant folding",
        shape
    ));
    assert!(!message_matches_shape("Error: Division by zero", shape));
}

#[test]
fn test_message_matches_shape_brace_around_non_identifier_is_literal() {
    // The shape mentions braces; it must not therefore match everything.
    let shape = "The '{' that opened this is never closed by a '}'.";
    assert!(message_matches_shape(
        "The '{' that opened this is never closed by a '}'.",
        shape
    ));
    assert!(!message_matches_shape("something else entirely", shape));
}

#[test]
fn test_message_matches_shape_brace_run_is_literal() {
    // A brace pair with no identifier inside is text, not a substitution point.
    let shape = "use system.io.{... as ...}";
    assert!(message_matches_shape("use system.io.{... as ...}", shape));
    assert!(!message_matches_shape("use system.io.{anything}", shape));
}

#[test]
fn test_message_matches_shape_newline_escape() {
    let shape = "Expected {thing}\nFound {other}";
    assert!(message_matches_shape("Expected int\nFound string", shape));
    assert!(!message_matches_shape("Expected int Found string", shape));
}

#[test]
fn test_message_matches_shape_placeholder_across_newline() {
    let shape = "Message: {content}";
    assert!(message_matches_shape("Message: line 1\nline 2", shape));
}

#[test]
fn test_normalize_rust_string_literals_line_continuation() {
    let source = "\"hello \\\n  world\"";
    let result = normalize_rust_string_literals(source);
    assert_eq!(result, "\"hello world\"");
}

#[test]
fn test_normalize_rust_string_literals_escaped_backslash() {
    let source = "\"hello \\\\\n  world\"";
    let result = normalize_rust_string_literals(source);
    // First backslash escapes second, so newline is NOT a continuation
    assert_eq!(result, "\"hello \\\\\n  world\"");
}

#[test]
fn test_normalize_rust_string_literals_no_continuation() {
    let source = "\"hello world\"";
    let result = normalize_rust_string_literals(source);
    assert_eq!(result, "\"hello world\"");
}
