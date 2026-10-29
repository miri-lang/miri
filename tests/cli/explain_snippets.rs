// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Proves that the example in each diagnostic's documentation really produces
//! that diagnostic.
//!
//! A documented example that does not reproduce its own error is worse than no
//! example: a reader copies it, sees something else, and stops trusting the
//! page. Every live code therefore sits in exactly one list below: either its
//! example is checked here against the real compiler, through the command that
//! raises the code (`check`, `run`, or another command with its flags), or it
//! carries a written reason why no source file can produce it. A newly added
//! code that appears in no list fails the partition test, so the lists cannot
//! quietly fall behind the registry, and the excused list has a ceiling that
//! only comes down.

use miri::diagnostics::DiagnosticCode;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Codes whose `Before` example is verified through `miri check` to emit
/// that code first.
const VERIFIED: &[&str] = &[
    "MER_IMP_002",
    "MER_IMP_003",
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
    "MER_MIR_016",
    "MER_MIR_017",
    "MER_MIR_018",
    "MER_NAM_001",
    "MER_NAM_002",
    "MER_OWN_001",
    "MER_OWN_003",
    "MER_OWN_004",
    "MER_OWN_005",
    "MER_PAR_001",
    "MER_PAR_002",
    "MER_PAR_003",
    "MER_PAR_004",
    "MER_PAR_005",
    "MER_PAR_009",
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
    "MER_TAR_001",
    "MER_TAR_002",
    "MER_TAR_003",
    "MER_TAR_004",
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
    "MER_TYP_019",
    "MER_TYP_021",
    "MER_TYP_022",
    "MER_TYP_023",
    "MER_TYP_024",
    "MER_TYP_025",
    "MER_TYP_026",
    "MER_TYP_027",
    "MER_TYP_029",
    "MER_TYP_030",
    "MER_TYP_031",
    "MER_TYP_032",
    "MER_TYP_033",
    "MER_TYP_034",
    "MER_TYP_035",
    "MER_TYP_036",
    "MER_TYP_037",
    "MER_TYP_038",
    "MER_TYP_039",
    "MER_TYP_040",
    "MER_TYP_041",
    "MER_TYP_042",
    "MER_TYP_043",
    "MER_TYP_044",
    "MER_TYP_047",
    "MER_TYP_048",
    "MER_TYP_049",
    "MER_TYP_050",
    "MER_TYP_051",
    "MER_TYP_052",
    "MER_TYP_053",
    "MER_TYP_054",
    "MER_TYP_055",
    "MER_TYP_056",
    "MER_TYP_057",
    "MER_TYP_058",
    "MER_TYP_059",
    "MER_TYP_060",
    "MER_TYP_062",
    "MER_TYP_063",
    "MER_TYP_064",
    "MER_TYP_065",
    "MER_TYP_066",
    "MER_TYP_067",
    "MER_TYP_068",
    "MER_TYP_069",
    "MER_TYP_070",
    "MER_TYP_071",
    "MER_TYP_072",
    "MER_TYP_073",
    "MER_TYP_074",
    "MER_TYP_075",
    "MER_TYP_076",
    "MER_TYP_077",
    "MER_TYP_078",
    "MER_TYP_079",
    "MER_TYP_080",
    "MER_TYP_081",
    "MER_TYP_082",
    "MER_TYP_083",
];

/// Codes whose `Before` example is verified through `miri run`.
///
/// Most stop the running program — a trap, or a signal that kills it — which
/// a static check cannot observe, so the example is compiled and run and the
/// code must be the one the run reports. The rest are a warning or a note that
/// only compiling a program to run raises, such as a file with nothing to run.
const VERIFIED_BY_RUN: &[&str] = &[
    "MER_BLD_021",
    "MER_BLD_026",
    "MER_RT_001",
    "MER_RT_002",
    "MER_RT_003",
    "MER_RT_004",
    "MER_RT_005",
    "MER_RT_006",
    "MER_RT_008",
    "MER_RT_009",
    "MER_RT_010",
    "MER_RT_011",
    "MER_RT_012",
    "MER_RT_015",
    "MER_RT_016",
    "MER_RT_017",
];

/// Codes whose `Before` example is verified through `miri run` on a machine
/// with a GPU: the device is what refuses the launch, so a run without one
/// cannot reach the code.
const VERIFIED_BY_RUN_ON_GPU: &[&str] = &["MER_RT_013"];

/// Codes whose `Before` example is verified through another command, given
/// the file and the flags that make the request.
///
/// The page's `Before` spells the same command and flags, so a reader can
/// repeat it; the gate checks that it does.
const VERIFIED_BY_COMMAND: &[(&str, &str, &[&str])] = &[
    ("MER_BLD_004", "view", &["--fn", "lenght"]),
    ("MER_BLD_005", "view", &["--fn", "draw"]),
    (
        "MER_BLD_006",
        "view",
        &["--fn", "main", "--around", "// tally the results"],
    ),
    (
        "MER_BLD_007",
        "view",
        &["--fn", "main", "--around", "index"],
    ),
    (
        "MER_BLD_009",
        "patch",
        &[
            "--replace-in-fn",
            "main",
            "--old",
            "x + 1",
            "--new",
            "x * 2",
            "--expect-sha",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ],
    ),
    (
        "MER_BLD_010",
        "patch",
        &[
            "--replace-in-fn",
            "demo",
            "--old",
            "1 + 1",
            "--new",
            "2 + 2",
        ],
    ),
    (
        "MER_BLD_011",
        "patch",
        &[
            "--replace-in-fn",
            "add",
            "--old",
            "a + b",
            "--new",
            "a + true",
        ],
    ),
    (
        "MER_BLD_012",
        "patch",
        &["--replace-in-fn", "total", "--old", "sum + 1"],
    ),
    (
        "MER_BLD_017",
        "patch",
        &["--insert-fn", "helper", "--body-file", "-"],
    ),
    ("MER_BLD_022", "view", &["--type", "Circel"]),
    (
        "MER_BLD_023",
        "view",
        &["--fn", "Inventory.total_value", "--around", "float"],
    ),
    ("MER_BLD_025", "test", &[]),
    ("MER_BLD_028", "profile", &[]),
    ("MER_BLD_029", "profile", &[]),
    ("MER_TAR_010", "build", &["--target", "web-gpu"]),
    ("MER_CG_007", "build", &["--cpu-backend", "llvm"]),
];

/// Codes whose `Before` example is verified through another command only on
/// a host that cannot do what the command asks: `miri profile` has a sampler
/// on macOS and Linux, on aarch64 and x86-64, and refuses everywhere else.
const VERIFIED_BY_COMMAND_ON_UNPROFILED_HOST: &[(&str, &str, &[&str])] =
    &[("MER_BLD_027", "profile", &[])];

/// Codes whose `Before` example is verified through `miri fix`, with the
/// flags that ask it to apply.
const VERIFIED_BY_FIX: &[(&str, &[&str])] = &[
    ("MER_BLD_002", &["--apply", "--yes"]),
    ("MER_BLD_020", &["--apply", "--yes"]),
];

/// Codes no documented example can reach, each with the reason.
///
/// A code belongs here only when no source file raises it through any
/// command. The ceiling below keeps the list from growing.
const NOT_VERIFIABLE: &[(&str, &str)] = &[
    ("MER_BLD_024", "raised by the agent session when a message on stdin is not a Content-Length frame, not by compiling a source file"),
    ("MER_BLD_001", "raised by `miri explain` for a code the registry does not hold, not by any source file"),
    ("MER_BLD_003", "raised by comparing two builds of one file, and no known source builds differently twice, so there is no example to run"),
    ("MER_BLD_008", "reports a file the command could not read; a source file an example writes is always readable"),
    ("MER_BLD_013", "names a skill the build does not carry, which is a property of the binary rather than of any source file"),
    ("MER_BLD_014", "reports an installed skill file that has been edited since it was written, not a property of any source file"),
    ("MER_BLD_015", "reports a directory the skill command cannot write to, not a property of any source file"),
    ("MER_BLD_016", "reports a skill embedded in this build whose header cannot be read, not a property of any source file"),
    ("MER_BLD_018", "a guard against a formatter defect: no known source renders differently a second time, so there is no example to run"),
    ("MER_BLD_019", "a guard against a formatter defect: no known source makes the renderer drop a comment or a word, so there is no example to run"),
    ("MER_CG_001", "documented with prose: no program provokes a backend failure on purpose"),
    ("MER_CG_002", "documented with prose: no program provokes a backend failure on purpose"),
    ("MER_CG_003", "documented with prose: no program provokes a backend failure on purpose"),
    ("MER_CG_004", "documented with prose: no program provokes a backend failure on purpose"),
    ("MER_CG_005", "documented with prose: no program provokes a backend failure on purpose"),
    ("MER_CG_006", "documented with prose: no program provokes a backend failure on purpose"),
    ("MER_CG_008", "documented with prose: it reports a failure of the compiler itself, and no known program reproduces one"),
    ("MER_IMP_001", "needs two files that import each other; an example is a single file"),
    ("MER_LEX_009", "the lexer forms a regex token only when both quotes are present, so the check never fails; an unterminated regex is MER_LEX_001"),
    ("MER_LEX_010", "the lexer forms an f-string token only when both quotes are present, so the check never fails; an unterminated f-string is MER_LEX_001"),
    ("MER_MIR_001", "every expression the type checker accepts lowers; one lowering could not handle is a gap in the compiler, which it reports as MER_CG_008"),
    ("MER_MIR_002", "every statement the type checker accepts lowers; one lowering could not handle is a gap in the compiler, which it reports as MER_CG_008"),
    ("MER_MIR_003", "the type checker refuses an undefined name first (MER_TYP_034)"),
    ("MER_MIR_004", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_MIR_005", "the type checker refuses a `break` outside a loop first (MER_TAR_005)"),
    ("MER_MIR_006", "the type checker refuses a `continue` outside a loop first (MER_TYP_050)"),
    ("MER_MIR_007", "the parser refuses an invalid assignment target first (MER_PAR_004), and lowering's own check would be reported as MER_CG_008"),
    ("MER_MIR_008", "every operator the type checker accepts lowers; one lowering could not handle is a gap in the compiler, which it reports as MER_CG_008"),
    ("MER_MIR_009", "raised only for a range form the type checker accepts and lowering cannot iterate, and none is known; lowering's refusal would be reported as MER_CG_008 in any case"),
    ("MER_MIR_010", "the type checker refuses a launch with the wrong arguments first (MER_TYP_030)"),
    ("MER_MIR_011", "the type checker refuses an unknown type first (MER_TYP_043), and lowering's own check would be reported as MER_CG_008"),
    ("MER_MIR_012", "the type checker refuses a missing struct field first (MER_TYP_030)"),
    ("MER_MIR_013", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_MIR_014", "documented with prose rather than source: no program reproduces a backend or internal-consistency failure"),
    ("MER_NAM_003", "the parser builds an import path from names joined by dots, so the path never holds `/`, `\\` or `..`; such a path is refused by the parser first (MER_PAR_001)"),
    ("MER_OWN_002", "no source spelling produces a linear type: the parser never builds one, so no variable is ever tracked as linear"),
    ("MER_PAR_006", "every float token the lexer forms parses as a float (an exponent past the range reads as infinity), so the check never fails"),
    ("MER_PAR_007", "the lexer forms a string token only when both quotes are present, so the check never fails; an unterminated string is MER_LEX_001"),
    ("MER_PAR_008", "the lexer forms a boolean token only from `true` or `false`, so the check never fails; any other word is a name"),
    ("MER_RT_007", "a bus error needs a misaligned or unmapped access the compiler never emits, and SIGBUS has a different number on macOS and Linux, so no portable program raises it"),
    ("MER_RT_014", "documented with prose: reaching it means the type checker let through a call it should have refused, so no program it handles correctly reproduces it"),
    ("MER_TYP_020", "the type checker refuses an unbounded `slice` range first (MER_TYP_041)"),
];

/// The most codes `NOT_VERIFIABLE` may hold.
///
/// Lowering this number is the only allowed direction: a code leaves the list
/// when an example reaches it, and the ceiling follows it down. Raising it
/// would hide a page whose example nobody checks.
const NOT_VERIFIABLE_CEILING: usize = 42;

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

/// The command a documented example is handed to.
#[derive(Clone, Copy, Debug)]
enum Surface {
    /// `miri check`: everything decided before code generation.
    Check,
    /// `miri run`: what stops the program while it runs.
    Run,
    /// Another command, with the flags that make the request.
    Command(&'static str, &'static [&'static str]),
    /// `miri fix`, which lists the errors it read before its verdict on them,
    /// so the code under test is the one it reports after them.
    Fix(&'static [&'static str]),
}

impl Surface {
    /// The subcommand this surface names.
    fn command(self) -> &'static str {
        match self {
            Surface::Check => "check",
            Surface::Run => "run",
            Surface::Command(command, _) => command,
            Surface::Fix(_) => "fix",
        }
    }

    /// The flags the request carries beyond the file.
    fn flags(self) -> &'static [&'static str] {
        match self {
            Surface::Check | Surface::Run => &[],
            Surface::Command(_, flags) | Surface::Fix(flags) => flags,
        }
    }

    /// The arguments that hand `path` to this command, asking for JSON.
    fn arguments(self, path: &str) -> Vec<&str> {
        let mut arguments = vec![self.command(), path];
        arguments.extend_from_slice(self.flags());
        arguments.extend(["--format", "json"]);
        arguments
    }

    /// Whether the command can finish with `status` and still have printed
    /// its envelope. `check` exits 0 or 1, and the test runner exits 2 when it
    /// has no verdict to give. A run exits with its program's status: 1 for a
    /// trap, 128 plus the number of a signal that killed it.
    fn finishes_with(self, status: i32) -> bool {
        match self {
            Surface::Check | Surface::Fix(_) => matches!(status, 0 | 1),
            Surface::Command(..) => matches!(status, 0..=2),
            Surface::Run => matches!(status, 0 | 1 | 129..=159),
        }
    }

    /// Where the diagnostic answering for `wire` sits: first, or for `fix`
    /// wherever the code is, since its verdict follows the errors it read.
    fn subject_index(self, reported: &[Diagnostic], wire: &str) -> Option<usize> {
        match self {
            Surface::Check | Surface::Run | Surface::Command(..) => {
                (!reported.is_empty()).then_some(0)
            }
            Surface::Fix(_) => reported
                .iter()
                .position(|d| d.code.as_deref() == Some(wire)),
        }
    }

    /// Where the code must appear in what the command reports, in words.
    fn expected_position(self) -> &'static str {
        match self {
            Surface::Check | Surface::Run | Surface::Command(..) => "first",
            Surface::Fix(_) => "after the errors it read",
        }
    }
}

/// One diagnostic a command reported.
struct Diagnostic {
    code: Option<String>,
    message: Option<String>,
    help: Option<String>,
}

/// Hand one source file to `surface` and return the JSON envelope it prints.
///
/// The file is written inside the repository because module resolution is
/// relative to the working directory: the same source checked from elsewhere
/// fails to find the standard library and reports that instead of the error
/// under test.
///
/// Each check writes its own file. Several gates check the same example at
/// once, and a shared file is truncated by one writer while another check is
/// reading it — the compiler then sees an empty program and reports nothing.
///
/// A run that prints no envelope (a crash, a signal, an exit status the
/// command never uses) is an `Err` carrying the status and stderr, so a broken
/// run is never mistaken for an example that reports no diagnostic.
fn snippet_envelope(
    source: &str,
    slot: &str,
    surface: Surface,
) -> Result<serde_json::Value, String> {
    static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/explain-snippets");
    std::fs::create_dir_all(&dir).expect("could not create the snippet directory");
    let unique = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{}-{}-{}.mi", slot, std::process::id(), unique));
    std::fs::write(&path, source).expect("could not write the snippet");

    let binary = PathBuf::from(env!("CARGO_BIN_EXE_miri"));
    let shown = path.display().to_string();
    let output = Command::new(binary)
        .args(surface.arguments(&shown))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    let _ = std::fs::remove_file(&path);
    envelope_of(
        &output.map_err(|e| format!("could not run the compiler: {}", e))?,
        surface,
    )
}

/// The envelope a finished command printed, or a description of a run that
/// printed none: its exit status, stdout and stderr.
fn envelope_of(output: &Output, surface: Surface) -> Result<serde_json::Value, String> {
    let envelope = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
    match (output.status.code(), envelope) {
        (Some(status), Some(envelope)) if surface.finishes_with(status) => Ok(envelope),
        (_, _) => Err(format!(
            "`miri {}` printed no diagnostic envelope ({}); stdout: {:?}; stderr: {:?}",
            surface.command(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}

/// Hand a source file to `surface` and read every diagnostic it reports, in
/// order.
fn report_of(source: &str, slot: &str, surface: Surface) -> Result<Vec<Diagnostic>, String> {
    let envelope = snippet_envelope(source, slot, surface)?;
    let text_of = |d: &serde_json::Value, field: &str| d[field].as_str().map(str::to_string);
    Ok(envelope["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| Diagnostic {
            code: text_of(d, "code"),
            message: text_of(d, "message"),
            help: text_of(d, "help"),
        })
        .collect())
}

/// All diagnostic codes reported by `miri check` for one source file.
fn codes_reported_for(source: &str, slot: &str) -> Result<Vec<String>, String> {
    Ok(codes_of(&report_of(source, slot, Surface::Check)?))
}

/// The codes a report carries, in order.
fn codes_of(reported: &[Diagnostic]) -> Vec<String> {
    reported.iter().filter_map(|d| d.code.clone()).collect()
}

/// Every code whose documented example a host gate verifies, with the command
/// it is verified through.
fn verified_on_host() -> impl Iterator<Item = (&'static str, Surface)> {
    let checked = VERIFIED.iter().map(|wire| (*wire, Surface::Check));
    let run = VERIFIED_BY_RUN.iter().map(|wire| (*wire, Surface::Run));
    let commanded = VERIFIED_BY_COMMAND
        .iter()
        .map(|(wire, command, flags)| (*wire, Surface::Command(command, flags)));
    let fixed = VERIFIED_BY_FIX
        .iter()
        .map(|(wire, flags)| (*wire, Surface::Fix(flags)));
    let unprofiled = VERIFIED_BY_COMMAND_ON_UNPROFILED_HOST
        .iter()
        .filter(|_| !miri::cli::profile::HOST_HAS_SAMPLER)
        .map(|(wire, command, flags)| (*wire, Surface::Command(command, flags)));
    checked
        .chain(run)
        .chain(commanded)
        .chain(fixed)
        .chain(unprofiled)
}

/// Every code whose documented example some gate verifies, on any machine.
fn verified_anywhere() -> impl Iterator<Item = &'static str> {
    let on_gpu = VERIFIED_BY_RUN_ON_GPU.iter().copied();
    let unprofiled = VERIFIED_BY_COMMAND_ON_UNPROFILED_HOST
        .iter()
        .map(|(wire, _, _)| *wire)
        .filter(|_| miri::cli::profile::HOST_HAS_SAMPLER);
    verified_on_host()
        .map(|(wire, _)| wire)
        .chain(on_gpu)
        .chain(unprofiled)
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
        let verified = verified_anywhere().filter(|gated| *gated == wire).count();
        let excused = NOT_VERIFIABLE.iter().filter(|(c, _)| *c == wire).count();
        assert!(
            verified + excused == 1,
            "{} must be in exactly one list: its example is either checked \
             against the compiler through one command, or it states why it cannot be",
            wire
        );
    }
}

/// The unverifiable list may shrink, never grow.
#[test]
fn the_unverifiable_list_stays_under_its_ceiling() {
    assert!(
        NOT_VERIFIABLE.len() <= NOT_VERIFIABLE_CEILING,
        "NOT_VERIFIABLE holds {} codes, past its ceiling of {}: give the new code \
         an example some command reaches instead of an excuse",
        NOT_VERIFIABLE.len(),
        NOT_VERIFIABLE_CEILING
    );
}

/// Panic with every failure a gate collected, or return when there is none.
fn assert_no_failures(what: &str, failures: Vec<String>) {
    if !failures.is_empty() {
        panic!("{} {}:\n  {}", failures.len(), what, failures.join("\n  "));
    }
}

/// What the `Before` example on one page reports through its command.
struct Example {
    code: DiagnosticCode,
    codes: Vec<String>,
    subject: Option<Diagnostic>,
}

/// Hand the `Before` example on `wire`'s page to `surface`, and read what it
/// reports.
fn reported_example(wire: &str, surface: Surface) -> Result<Example, String> {
    let code: DiagnosticCode = wire
        .parse()
        .map_err(|_| format!("{} is not a registered code", wire))?;
    let example = before_example(code).ok_or_else(|| format!("{} has no Before example", wire))?;
    let reported =
        report_of(&example, wire, surface).map_err(|run| format!("{}: {}", wire, run))?;
    let codes = codes_of(&reported);
    let subject = surface
        .subject_index(&reported, wire)
        .and_then(|index| reported.into_iter().nth(index));
    Ok(Example {
        code,
        codes,
        subject,
    })
}

/// What is wrong with one gated example: it must report its own code where
/// its command puts it, in a message one of its page's declared shapes
/// matches, and its page must spell the command it is verified through.
fn example_failures(wire: &str, surface: Surface) -> Vec<String> {
    let example = match reported_example(wire, surface) {
        Ok(found) => found,
        Err(failure) => return vec![failure],
    };
    let mut failures: Vec<String> = page_spelling_failure(example.code, surface)
        .into_iter()
        .collect();
    let subject = example.subject.as_ref();
    if subject.and_then(|d| d.code.as_deref()) != Some(wire) {
        failures.push(format!(
            "{}: `miri {}` on the documented example must report it {}; it reported {:?}",
            wire,
            surface.command(),
            surface.expected_position(),
            example.codes
        ));
    }
    let shapes = example.code.explanation().messages;
    match subject.and_then(|d| d.message.as_deref()) {
        _ if shapes.is_empty() => failures.push(format!(
            "{} has no ## Messages section documenting the shapes this code can emit. \
             Add a ## Messages section with backticked message shapes.",
            wire
        )),
        None => failures.push(no_subject(wire, &example.codes)),
        Some(message)
            if !shapes
                .iter()
                .any(|shape| message_matches_shape(message, shape)) =>
        {
            failures.push(format!(
                "{}: the first diagnostic message '{}' does not match any declared shape: {:?}",
                wire, message, shapes
            ))
        }
        Some(_) => {}
    }
    failures
}

/// A page verified through a command other than `check` or `run` must spell
/// that command and every flag in its `Before`, so a reader repeating it gets
/// what the gate got.
fn page_spelling_failure(code: DiagnosticCode, surface: Surface) -> Option<String> {
    if matches!(surface, Surface::Check | Surface::Run) {
        return None;
    }
    let doc = code.doc();
    let before = doc
        .split("## Before")
        .nth(1)
        .and_then(|rest| rest.split("## After").next())
        .unwrap_or_default();
    let invocation = format!("miri {}", surface.command());
    let missing: Vec<&str> = std::iter::once(invocation.as_str())
        .chain(surface.flags().iter().copied())
        .filter(|part| !before.contains(part))
        .collect();
    (!missing.is_empty()).then(|| {
        format!(
            "{}: its Before section must spell the command it is verified through; it lacks {:?}",
            code.as_str(),
            missing
        )
    })
}

#[test]
fn verified_examples_emit_the_code_they_document() {
    let failures = verified_on_host()
        .flat_map(|(wire, surface)| example_failures(wire, surface))
        .collect();
    assert_no_failures("verified codes failed", failures);
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
    "MER_RT_001",
    "MER_RT_002",
    "MER_RT_003",
    "MER_RT_004",
    "MER_RT_005",
    "MER_RT_006",
    "MER_RT_008",
    "MER_RT_009",
    "MER_RT_010",
    "MER_RT_011",
    "MER_RT_012",
    "MER_RT_013",
    "MER_RT_015",
    "MER_RT_016",
    "MER_RT_017",
    "MER_TAR_001",
    "MER_TAR_002",
    "MER_TAR_004",
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
    "MER_TYP_035",
    "MER_TYP_036",
    "MER_TYP_037",
    "MER_TYP_040",
    "MER_TYP_041",
    "MER_TYP_042",
    "MER_TYP_043",
    "MER_TYP_044",
    "MER_TYP_047",
    "MER_TYP_048",
    "MER_TYP_049",
    "MER_TYP_050",
    "MER_TYP_052",
    "MER_TYP_053",
    "MER_TYP_054",
    "MER_TYP_055",
    "MER_TYP_056",
    "MER_TYP_059",
    "MER_TYP_060",
    "MER_TYP_062",
    "MER_TYP_064",
    "MER_TYP_065",
    "MER_TYP_066",
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
    let failures = verified_on_host()
        .flat_map(|(wire, surface)| help_failures(wire, surface))
        .collect();
    assert_no_failures("pages disagree with the help that fires", failures);
}

/// The failure for a run whose envelope held no diagnostic with a message to
/// judge, naming what it did report so a flaky run says what happened rather
/// than only that extraction failed.
fn no_subject(wire: &str, reported: &[String]) -> String {
    format!(
        "{}: the run reported no diagnostic with a message to judge; it reported {:?}",
        wire, reported
    )
}

/// What is wrong with the help one gated page declares, against the help its
/// example really reports.
fn help_failures(wire: &str, surface: Surface) -> Vec<String> {
    let example = match reported_example(wire, surface) {
        Ok(found) => found,
        Err(failure) => return vec![failure],
    };
    let Some(subject) = example.subject.filter(|d| d.message.is_some()) else {
        return vec![no_subject(wire, &example.codes)];
    };
    let declared = example.code.explanation().helps;
    let silent = NO_HELP_EMITTED.contains(&wire);
    match subject.help {
        Some(help) if silent => vec![format!(
            "{} is listed as emitting no help, but it emitted '{}'; \
             remove it from NO_HELP_EMITTED and declare the help on its page",
            wire, help
        )],
        Some(help) if declared.is_empty() => vec![format!(
            "{} emits the help '{}' but its page declares none. \
             Add a ## Help section carrying it as a backticked shape.",
            wire, help
        )],
        Some(help)
            if !declared
                .iter()
                .any(|shape| message_matches_shape(&help, shape)) =>
        {
            vec![format!(
                "{}: the help that fires, '{}', matches no shape its page declares: {:?}",
                wire, help, declared
            )]
        }
        Some(_) => Vec::new(),
        None => silent_help_failures(wire, silent, &declared),
    }
}

/// What is wrong with a page whose example reports no help: the code must be
/// listed as silent, and the page must declare no help nothing proves.
fn silent_help_failures(wire: &str, silent: bool, declared: &[String]) -> Vec<String> {
    let mut failures = Vec::new();
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
    failures
}

/// The GPU's own refusals are verified like every other run, on a machine
/// that has one.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_examples_emit_the_code_and_help_they_document() {
    let failures = VERIFIED_BY_RUN_ON_GPU
        .iter()
        .flat_map(|wire| {
            let mut failures = example_failures(wire, Surface::Run);
            failures.extend(help_failures(wire, Surface::Run));
            failures
        })
        .collect();
    assert_no_failures("GPU examples failed", failures);
}

/// A code listed as silent must be a live code whose page is verified.
#[test]
fn every_silent_code_is_verified() {
    for wire in NO_HELP_EMITTED {
        assert!(
            verified_anywhere().any(|gated| gated == *wire),
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
                .is_some_and(|n| n == "target")
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

    for wire in verified_anywhere() {
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
        let reported =
            codes_reported_for(&example, wire).unwrap_or_else(|run| panic!("{}: {}", wire, run));
        assert!(
            !reported.iter().any(|c| c == wire),
            "{} is on the excused list, but its example now reports it ({:?}); \
             move it to the verified list",
            wire,
            reported
        );
    }
}

/// Several gates check the same documented example at the same moment, so a
/// check must read the whole source no matter what the others are doing. A
/// check that read a file another check was rewriting saw an empty program,
/// which reports nothing and read as "the example emits no code".
#[test]
fn concurrent_checks_of_one_example_each_see_the_whole_source() {
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Rewrites the file a shared slot would use, as a sibling gate checking
    // the same example does.
    let rewriter = {
        let done = done.clone();
        std::thread::spawn(move || {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/explain-snippets");
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = std::fs::write(dir.join("MER_PAR_002.mi"), "let x = (\n");
            }
        })
    };
    let workers: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                (0..10)
                    .map(|_| {
                        codes_reported_for("let x = (\n", "MER_PAR_002")
                            .unwrap_or_else(|run| panic!("{}", run))
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .flat_map(|worker| worker.join().expect("a checking thread panicked"))
        .collect();
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    rewriter.join().expect("the rewriting thread panicked");
    for reported in results {
        assert_eq!(reported.first().map(String::as_str), Some("MER_PAR_002"));
    }
}

/// A compiler run that dies prints no envelope; the gate must say so, with
/// the run's stderr, rather than read the silence as "no diagnostics".
#[cfg(unix)]
#[test]
fn a_killed_check_is_reported_with_its_stderr() {
    use std::os::unix::process::ExitStatusExt;
    let killed = Output {
        status: std::process::ExitStatus::from_raw(9),
        stdout: Vec::new(),
        stderr: b"thread 'main' has overflowed its stack".to_vec(),
    };
    let Err(report) = envelope_of(&killed, Surface::Check) else {
        panic!("a killed run must not yield an envelope");
    };
    assert!(report.contains("signal: 9"), "{}", report);
    assert!(report.contains("overflowed its stack"), "{}", report);
}

/// A run that exits with a status `check` never uses is broken even when it
/// printed something that parses.
#[cfg(unix)]
#[test]
fn a_check_exiting_outside_its_statuses_is_reported() {
    use std::os::unix::process::ExitStatusExt;
    let crashed = Output {
        status: std::process::ExitStatus::from_raw(101 << 8),
        stdout: br#"{"diagnostics": []}"#.to_vec(),
        stderr: b"panicked at src/main.rs".to_vec(),
    };
    assert!(envelope_of(&crashed, Surface::Check).is_err());
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
