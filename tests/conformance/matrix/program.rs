// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Assembles many cells into one Miri program.
//!
//! Each cell gets its own items, a driver function that builds its two
//! values, runs the operation and prints one sentinel line, and a recorded
//! line range so a diagnostic can be charged to the cell it points into.

use super::cells::{Cell, Expected, Operation, Outcome};
use super::source::cell_source;
use super::types::{Decl, Value};
use std::collections::HashSet;
use std::ops::RangeInclusive;

/// Printed first by `main`, so its absence means the program never ran.
pub const STARTED: &str = "@@started";

/// Imports every program carries. An unused import only warns.
const IMPORTS: &str = "use system.collections.list\nuse system.collections.set\nuse system.collections.map\nuse system.collections.array\n";

/// A program built from a batch of cells.
pub struct Program {
    pub source: String,
    /// The source lines (1-based) belonging to each cell, in batch order.
    pub ranges: Vec<RangeInclusive<usize>>,
}

/// The sentinel line prefix a cell's driver prints before its observation.
pub fn sentinel(id: usize) -> String {
    format!("@@{id}|")
}

/// The full line a correct cell prints, or `None` for a refused cell.
pub fn expected_line(cell: &Cell, id: usize) -> Option<String> {
    let observed = match &cell.outcome {
        Outcome::Refused(_) => return None,
        Outcome::Runs(Expected::Count) => "1".to_string(),
        Outcome::Runs(Expected::Text(text)) => text.to_string(),
        Outcome::Runs(Expected::Value(value)) => match value.twin {
            Some(_) => format!("{} true false", value.shown),
            None => value.shown.to_string(),
        },
    };
    Some(format!("{}{observed}", sentinel(id)))
}

pub fn assemble(cells: &[&Cell]) -> Program {
    let mut source = String::from(IMPORTS);
    for decl in shared_decls(cells) {
        source.push('\n');
        source.push_str(decl.text);
    }
    let mut ranges = Vec::with_capacity(cells.len());
    for (id, cell) in cells.iter().enumerate() {
        source.push('\n');
        let first = source.lines().count() + 1;
        let cell_source = cell_source(cell, id);
        source.push_str(&cell_source.items);
        source.push_str(&driver(cell, id, &cell_source.call));
        ranges.push(first..=source.lines().count());
    }
    source.push_str(&format!("\nfn main()\n    println(\"{STARTED}\")\n"));
    for id in 0..cells.len() {
        source.push_str(&format!("    println(drive{id}())\n"));
    }
    Program { source, ranges }
}

/// The type declarations the batch's element types need, each once, with
/// imports first so they head the program.
fn shared_decls(cells: &[&Cell]) -> Vec<Decl> {
    let mut seen = HashSet::new();
    let mut decls: Vec<Decl> = cells
        .iter()
        .flat_map(|cell| cell.ty.decls.iter().copied())
        .filter(|decl| seen.insert(decl.name))
        .collect();
    decls.sort_by_key(|decl| !decl.text.starts_with("use "));
    decls
}

/// A function that builds the cell's values, runs it and returns the line
/// `main` prints for it.
///
/// Returning the line rather than printing it is what lets a crash be charged
/// by its position: every value the driver made is released when it returns,
/// before its line is printed, so a violation in that release stops the program
/// with this cell's line still missing — not with the next cell's.
fn driver(cell: &Cell, id: usize, call: &str) -> String {
    let ty = cell.ty.spelling;
    let (a, b) = if cell.operation == Operation::Dedup {
        cell.ty.equal_pair
    } else {
        (cell.ty.a.expr, cell.ty.b.expr)
    };
    let mut text = format!(
        "fn drive{id}() String\n    let a {ty} = {a}\n    let b {ty} = {b}\n    let ea = a\n    let eb = b\n    let r = {call}\n"
    );
    let observation = match &cell.outcome {
        Outcome::Runs(Expected::Value(value)) => {
            text.push_str(&format!(
                "    let o = {}\n",
                cell.ty.observe.replace("$r", "r")
            ));
            twin_check(&mut text, ty, value)
        }
        Outcome::Runs(Expected::Count) => {
            text.push_str("    let o = f\"{r}\"\n");
            "{o}"
        }
        // A refused cell never runs; its driver only has to reach the call.
        Outcome::Refused(_) => "refused",
        Outcome::Runs(Expected::Text(_)) => {
            text.push_str("    let o = r\n");
            "{o}"
        }
    };
    text.push_str(&format!("    return f\"{}{observation}\"\n", sentinel(id)));
    text
}

/// For a value with a low-word twin, appends a comparison against the value
/// and against its twin, and returns the observation including both.
fn twin_check(text: &mut String, ty: &str, value: &Value) -> &'static str {
    match value.twin {
        Some(twin) => {
            text.push_str(&format!(
                "    let e {ty} = {}\n    let w {ty} = {twin}\n    let t = f\"{{r == e}} {{r == w}}\"\n",
                value.expr
            ));
            "{o} {t}"
        }
        None => "{o}",
    }
}
