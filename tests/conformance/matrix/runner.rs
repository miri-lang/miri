// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Runs batches of cells and charges every failure to the cell that caused it.
//!
//! A batch is one program, and starting a program is the expensive part, so a
//! failure is charged from its evidence wherever the evidence allows:
//!
//! - a diagnostic with a location names the cell whose lines it points into;
//! - an unlocated internal error names the function it failed in, and every
//!   function a cell declares carries the cell's number;
//! - a crash names the first cell that printed nothing, because the heap
//!   guard stops at the violation and every value a driver makes is released
//!   before the driver returns.
//!
//! Only a failure with no such evidence — a leak reported at exit, a compiler
//! panic — splits the batch in two. The charged cells are removed and the
//! rest is judged again, until every cell has a verdict.

use super::cells::{Cell, Outcome};
use super::program::{assemble, expected_line, sentinel, Program, STARTED};
use super::source::ITEM_PREFIXES;
use crate::utils::miri_cmd;
use regex::Regex;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use tempfile::NamedTempFile;

/// A program that runs longer than this is treated as hung.
const TIMEOUT: Duration = Duration::from_secs(300);

/// A red cell's evidence, one line.
pub type Reason = String;

type Verdicts = Vec<(String, Reason)>;

/// What happened to one program.
struct Run {
    file_name: String,
    exit_ok: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    fn started(&self) -> bool {
        self.stdout.lines().any(|line| line == STARTED)
    }

    fn is_clean(&self) -> bool {
        self.exit_ok
            && !self.stderr.contains("MIRI_LEAK_CHECK:")
            && !self.stderr.contains("MIRI_HEAP_GUARD:")
    }
}

/// The error codes a failed compile charged to one cell, and the line that
/// explains an unlocated one.
#[derive(Default)]
struct Charge {
    codes: Vec<String>,
    detail: Option<String>,
}

impl Charge {
    fn reason(&self) -> String {
        let codes = self.codes.join(",");
        match &self.detail {
            Some(detail) => format!("compile error {codes}: {detail}"),
            None => format!("compile error {codes}"),
        }
    }
}

/// Judges every cell, returning the red ones with their reason.
pub fn red_cells(cells: &[Cell]) -> Verdicts {
    let (runs, refusals): (Vec<&Cell>, Vec<&Cell>) = cells
        .iter()
        .partition(|cell| matches!(cell.outcome, Outcome::Runs(_)));
    let mut red = Vec::new();
    judge_runs(&runs, &mut red);
    judge_refusals(&refusals, &mut red);
    for (_, reason) in &mut red {
        *reason = tidy(reason);
    }
    red.sort();
    red
}

/// Strips what differs between two runs of the same failure — absolute and
/// temporary paths, thread ids, allocation sequence numbers, the cell's number in its batch —
/// so a reason reads the same every time it is printed.
fn tidy(reason: &str) -> String {
    static NOISE: OnceLock<[(Regex, &str); 5]> = OnceLock::new();
    let noise = NOISE.get_or_init(|| {
        let rule = |pattern: &str, replacement| {
            (
                Regex::new(pattern).expect("a reason-tidying pattern is valid"),
                replacement,
            )
        };
        [
            rule(
                r"thread '[^']*' \(\d+\) panicked at \S*/([^/\s]+:\d+):\d+:",
                "compiler panicked at $1:",
            ),
            rule(r" ?\((?:alloc )?seq=\d+\)", ""),
            rule(r"\S*/\.tmp\w+\.mi", "<program>"),
            rule(
                r"\b(cell|drive|get|pick|pass|lit|Cell|Base|Sub|Op|Impl|HB|HC|H|K)\d+",
                "${1}N",
            ),
            rule(r"\s+", " "),
        ]
    });
    noise
        .iter()
        .fold(reason.to_string(), |text, (pattern, replacement)| {
            pattern.replace_all(&text, *replacement).into_owned()
        })
}

fn judge_runs(cells: &[&Cell], red: &mut Verdicts) {
    if cells.is_empty() {
        return;
    }
    let program = assemble(cells);
    let run = execute(&program);
    if !run.started() {
        let charges = compile_charges(&program, &run);
        if charges.is_empty() {
            return split_or_charge(cells, red, judge_runs, || first_error(&run));
        }
        for (&id, charge) in &charges {
            red.push((cells[id].name.clone(), charge.reason()));
        }
        return judge_runs(&keep_uncharged(cells, &charges), red);
    }
    let printed = printed_lines(&run.stdout, cells.len());
    if let Some(crashed) = (0..cells.len()).find(|id| !printed.contains_key(id)) {
        red.push((cells[crashed].name.clone(), crash_reason(&run)));
        let rest: Vec<&Cell> = cells
            .iter()
            .enumerate()
            .filter(|&(id, _)| id != crashed)
            .map(|(_, cell)| *cell)
            .collect();
        return judge_runs(&rest, red);
    }
    if !run.is_clean() {
        return split_or_charge(cells, red, judge_runs, || crash_reason(&run));
    }
    for (id, cell) in cells.iter().enumerate() {
        if let Some(reason) = wrong_value(cell, id, &printed[&id]) {
            red.push((cell.name.clone(), reason));
        }
    }
}

fn wrong_value(cell: &Cell, id: usize, got: &str) -> Option<Reason> {
    let expected = expected_line(cell, id).unwrap_or_default();
    if got == expected {
        return None;
    }
    let prefix = sentinel(id);
    Some(format!(
        "wrong value: got `{}`, expected `{}`",
        got.trim_start_matches(&prefix),
        expected.trim_start_matches(&prefix)
    ))
}

/// A refused cell is green when the compiler refuses it with its code on one
/// of its own lines. Refusals are judged in rounds: each round settles every
/// cell a diagnostic was charged to and re-batches the rest, which may have
/// been hidden behind an earlier phase's errors.
fn judge_refusals(cells: &[&Cell], red: &mut Verdicts) {
    if cells.is_empty() {
        return;
    }
    let program = assemble(cells);
    let run = execute(&program);
    if run.started() {
        for cell in cells {
            red.push((
                cell.name.clone(),
                format!("accepted; expected refusal {}", refusal_code(cell)),
            ));
        }
        return;
    }
    let charges = compile_charges(&program, &run);
    if charges.is_empty() {
        return split_or_charge(cells, red, judge_refusals, || {
            format!(
                "expected refusal {}; {}",
                refusal_code(cells[0]),
                first_error(&run)
            )
        });
    }
    for (&id, charge) in &charges {
        let code = refusal_code(cells[id]);
        if !charge.codes.iter().any(|charged| charged == code) {
            red.push((
                cells[id].name.clone(),
                format!("expected refusal {code}; {}", charge.reason()),
            ));
        }
    }
    judge_refusals(&keep_uncharged(cells, &charges), red);
}

fn refusal_code(cell: &Cell) -> &'static str {
    match cell.outcome {
        Outcome::Refused(code) => code,
        Outcome::Runs(_) => "none",
    }
}

/// Charges a lone cell with `reason`, or splits a batch in two — keeping each
/// element type's cells together while more than one type remains — and
/// judges both halves at once.
fn split_or_charge(
    cells: &[&Cell],
    red: &mut Verdicts,
    judge: fn(&[&Cell], &mut Verdicts),
    reason: impl FnOnce() -> Reason,
) {
    if let [cell] = cells {
        return red.push((cell.name.clone(), reason()));
    }
    let middle = cells.len() / 2;
    let boundary = (1..cells.len())
        .filter(|&i| cells[i].ty.token != cells[i - 1].ty.token)
        .min_by_key(|&i| i.abs_diff(middle))
        .unwrap_or(middle);
    let (front, back) = cells.split_at(boundary);
    let back_red = std::thread::scope(|scope| {
        let back_half = scope.spawn(|| {
            let mut back_red = Vec::new();
            judge(back, &mut back_red);
            back_red
        });
        judge(front, red);
        back_half.join().expect("a matrix batch half panicked")
    });
    red.extend(back_red);
}

fn keep_uncharged<'a>(cells: &[&'a Cell], charges: &BTreeMap<usize, Charge>) -> Vec<&'a Cell> {
    cells
        .iter()
        .enumerate()
        .filter(|(id, _)| !charges.contains_key(id))
        .map(|(_, cell)| *cell)
        .collect()
}

/// The sentinel lines a run printed, keyed by cell id.
fn printed_lines(stdout: &str, count: usize) -> BTreeMap<usize, String> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("@@")?;
            let id: usize = rest.split('|').next()?.parse().ok()?;
            (id < count).then(|| (id, line.to_string()))
        })
        .collect()
}

/// One error diagnostic: its code, its line in the program if it has one,
/// and the lines of its report.
struct Diagnostic {
    code: String,
    line: Option<usize>,
    report: Vec<String>,
}

/// Every error diagnostic the compiler printed.
fn diagnostics(run: &Run) -> Vec<Diagnostic> {
    let mut found: Vec<Diagnostic> = Vec::new();
    let mut in_error = false;
    for line in run.stderr.lines() {
        if let Some(rest) = line.strip_prefix("error[") {
            in_error = true;
            found.push(Diagnostic {
                code: rest.split(']').next().unwrap_or_default().to_string(),
                line: None,
                report: Vec::new(),
            });
        } else if line.starts_with("warning[") {
            in_error = false;
        } else if let (true, Some(current)) = (in_error, found.last_mut()) {
            if current.line.is_none() {
                current.line = program_line(line, &run.file_name);
            }
            current.report.push(line.trim().to_string());
        }
    }
    found
}

/// The line number a `--> path:line:column` location gives, when the path is
/// the program's own. The compiler prints the canonical path, which on macOS
/// differs from the temporary one by a `/private` prefix, so the unique file
/// name is what is compared.
fn program_line(line: &str, file_name: &str) -> Option<usize> {
    let location = line.trim_start().strip_prefix("--> ")?;
    let (file, position) = location.split_once(".mi:")?;
    if !format!("{file}.mi").ends_with(file_name) {
        return None;
    }
    position.split(':').next()?.parse().ok()
}

/// The cells each error diagnostic of a failed compile can be charged to.
fn compile_charges(program: &Program, run: &Run) -> BTreeMap<usize, Charge> {
    let mut charges: BTreeMap<usize, Charge> = BTreeMap::new();
    for diagnostic in diagnostics(run) {
        if let Some(line) = diagnostic.line {
            if let Some(id) = program.ranges.iter().position(|r| r.contains(&line)) {
                charges.entry(id).or_default().codes.push(diagnostic.code);
            }
            continue;
        }
        if spans_cells(&diagnostic.report) {
            continue;
        }
        for (id, detail) in named_cells(&diagnostic.report, program.ranges.len()) {
            let charge = charges.entry(id).or_default();
            charge.codes.push(diagnostic.code.clone());
            charge.detail.get_or_insert(detail);
        }
    }
    for charge in charges.values_mut() {
        charge.codes.sort_unstable();
        charge.codes.dedup();
    }
    charges
}

/// Whether a report is about two declarations disagreeing. A runtime symbol
/// is declared once per program, so the function that failed to declare it
/// may be an innocent cell whose declaration merely came second; such a batch
/// is split until each cell is judged on its own.
fn spans_cells(report: &[String]) -> bool {
    report
        .iter()
        .any(|line| line.contains("incompatible with previous declaration"))
}

/// The cells an unlocated report names through the functions it cites, with
/// the report line naming each.
fn named_cells(report: &[String], count: usize) -> Vec<(usize, String)> {
    static CELL_FUNCTION: OnceLock<Regex> = OnceLock::new();
    let pattern = CELL_FUNCTION.get_or_init(|| {
        let prefixes = ITEM_PREFIXES.join("|");
        Regex::new(&format!(
            r"(?:fn |function '|symbols?:? _?)(?:{prefixes})(\d+)"
        ))
        .expect("the cell-function pattern is valid")
    });
    let mut named = Vec::new();
    for line in report {
        for captures in pattern.captures_iter(line) {
            if let Some(id) = captures[1].parse().ok().filter(|&id| id < count) {
                named.push((id, line.chars().take(200).collect()));
            }
        }
    }
    named
}

/// The first error and the line under it, or the compiler's panic.
fn first_error(run: &Run) -> String {
    let lines: Vec<&str> = run.stderr.lines().collect();
    let found = lines
        .iter()
        .position(|line| line.starts_with("error") || line.contains("panicked at"));
    let text = match found {
        Some(at) => {
            let detail = lines[at + 1..]
                .iter()
                .map(|line| line.trim())
                .find(|line| !line.is_empty())
                .unwrap_or("");
            format!("{} {detail}", lines[at].trim())
        }
        None => "failed without a diagnostic".to_string(),
    };
    text.chars().take(240).collect()
}

fn crash_reason(run: &Run) -> String {
    let marker = run
        .stderr
        .lines()
        .find(|line| {
            line.contains("MIRI_HEAP_GUARD:")
                || line.contains("MIRI_LEAK_CHECK:")
                || line.contains("Runtime error")
                || line.contains("panicked at")
        })
        .unwrap_or("crashed without a report");
    marker.trim().chars().take(200).collect()
}

/// Compiles and runs a program with the leak check and the heap guard armed.
fn execute(program: &Program) -> Run {
    let mut file = NamedTempFile::with_suffix(".mi").expect("create a temporary source file");
    file.write_all(program.source.as_bytes())
        .expect("write the temporary source file");
    let file_name = file
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    keep_for_inspection(&program.source, &file_name);
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib");
    let output = miri_cmd()
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .env("MIRI_STDLIB_PATH", stdlib)
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .timeout(TIMEOUT)
        .arg("run")
        .arg(file.path())
        .output()
        .expect("spawn the miri compiler");
    Run {
        file_name,
        exit_ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// With `MIRI_MATRIX_KEEP` set, copies every program the matrix runs into
/// `target/conformance-matrix/programs/`, so a red cell can be reproduced by
/// hand. The file name matches the one diagnostics print.
fn keep_for_inspection(source: &str, file_name: &str) {
    if std::env::var_os("MIRI_MATRIX_KEEP").is_none() {
        return;
    }
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/conformance-matrix/programs");
    let _ = std::fs::create_dir_all(&directory);
    let _ = std::fs::write(directory.join(file_name), source);
}
