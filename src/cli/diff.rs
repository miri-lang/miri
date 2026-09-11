// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders the difference between two texts as a unified diff.
//!
//! Lines are matched by a longest common subsequence, so a change in two places
//! is reported as two hunks, each with a few unchanged lines around it for a
//! reader to find it by. A pair of texts too large to match line against line
//! is reported as one hunk replacing the stretch between their common first
//! and last lines, which is coarser and still a truthful account of what the
//! text becomes.

use std::ops::Range;

/// Unchanged lines shown either side of a change.
const CONTEXT: usize = 3;

/// The most cells a line-matching table may hold before the stretch that
/// differs is reported as replaced whole.
const MAXIMUM_TABLE_CELLS: usize = 4_000_000;

/// The marker a unified diff writes below a line with no line break after it.
const NO_NEWLINE_MARKER: &str = "\\ No newline at end of file\n";

/// What happened to one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Kept,
    Removed,
    Added,
}

/// One line of the edit script, with where each text's cursor stood before it.
#[derive(Debug, Clone, Copy)]
struct Step {
    change: Change,
    old: usize,
    new: usize,
}

/// Render the change from `before` to `after` as a unified diff labelled with
/// `label`, or a header alone when the two are the same.
pub fn unified_diff(label: &str, before: &str, after: &str) -> String {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let steps = edit_script(&old, &new);

    let mut diff = header(label);
    for hunk in hunks(&steps) {
        write_hunk(&mut diff, &steps[hunk], &old, &new);
    }
    diff
}

/// The `---` and `+++` lines naming the two sides.
fn header(label: &str) -> String {
    if label.starts_with('/') {
        format!("--- a{}\n+++ b{}\n", label, label)
    } else {
        format!("--- a/{}\n+++ b/{}\n", label, label)
    }
}

/// Every line of both texts, in order, marked kept, removed or added.
fn edit_script(old: &[&str], new: &[&str]) -> Vec<Step> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let mut steps: Vec<Step> = (0..prefix)
        .map(|line| Step {
            change: Change::Kept,
            old: line,
            new: line,
        })
        .collect();
    let old_middle = &old[prefix..old.len() - suffix];
    let new_middle = &new[prefix..new.len() - suffix];
    match lcs_table(old_middle, new_middle) {
        Some(table) => matched_steps(old_middle, new_middle, &table, prefix, &mut steps),
        None => replaced_steps(old_middle.len(), new_middle.len(), prefix, &mut steps),
    }
    steps.extend((0..suffix).map(|line| Step {
        change: Change::Kept,
        old: old.len() - suffix + line,
        new: new.len() - suffix + line,
    }));
    steps
}

/// The length of the longest common subsequence of every pair of suffixes,
/// row by row, or nothing when the table would be too large to build.
fn lcs_table(old: &[&str], new: &[&str]) -> Option<Vec<u32>> {
    let width = new.len() + 1;
    let cells = (old.len() + 1).checked_mul(width)?;
    if cells > MAXIMUM_TABLE_CELLS {
        return None;
    }
    let mut table = vec![0u32; cells];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i * width + j] = if old[i] == new[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    Some(table)
}

/// Walk the table from the start, keeping a common line where there is one and
/// removing before adding where both would keep as much.
fn matched_steps(old: &[&str], new: &[&str], table: &[u32], offset: usize, steps: &mut Vec<Step>) {
    let width = new.len() + 1;
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        let change = if i < old.len() && j < new.len() && old[i] == new[j] {
            Change::Kept
        } else if i < old.len()
            && (j == new.len() || table[(i + 1) * width + j] >= table[i * width + j + 1])
        {
            Change::Removed
        } else {
            Change::Added
        };
        steps.push(Step {
            change,
            old: offset + i,
            new: offset + j,
        });
        match change {
            Change::Kept => {
                i += 1;
                j += 1;
            }
            Change::Removed => i += 1,
            Change::Added => j += 1,
        }
    }
}

/// Every line of the stretch removed, then every line of its replacement added.
fn replaced_steps(removed: usize, added: usize, offset: usize, steps: &mut Vec<Step>) {
    steps.extend((0..removed).map(|line| Step {
        change: Change::Removed,
        old: offset + line,
        new: offset,
    }));
    steps.extend((0..added).map(|line| Step {
        change: Change::Added,
        old: offset + removed,
        new: offset + line,
    }));
}

/// The ranges of steps each hunk covers: every change, with its context, and
/// changes close enough that their context would touch merged into one.
fn hunks(steps: &[Step]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let changed = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.change != Change::Kept)
        .map(|(index, _)| index);
    for index in changed {
        let start = index.saturating_sub(CONTEXT);
        let end = (index + CONTEXT + 1).min(steps.len());
        match ranges.last_mut() {
            Some(last) if start <= last.end => last.end = end,
            _ => ranges.push(start..end),
        }
    }
    ranges
}

/// Write one hunk: its `@@` line, then each of its lines marked.
fn write_hunk(diff: &mut String, steps: &[Step], old: &[&str], new: &[&str]) {
    let Some(first) = steps.first() else {
        return;
    };
    let old_count = steps
        .iter()
        .filter(|step| step.change != Change::Added)
        .count();
    let new_count = steps
        .iter()
        .filter(|step| step.change != Change::Removed)
        .count();
    diff.push_str(&format!(
        "@@ -{} +{} @@\n",
        hunk_range(first.old, old_count),
        hunk_range(first.new, new_count)
    ));
    for step in steps {
        let (marker, line) = match step.change {
            Change::Kept => (' ', old[step.old]),
            Change::Removed => ('-', old[step.old]),
            Change::Added => ('+', new[step.new]),
        };
        diff.push(marker);
        diff.push_str(line);
        if !line.ends_with('\n') {
            diff.push('\n');
            diff.push_str(NO_NEWLINE_MARKER);
        }
    }
}

/// A hunk's `start,count`, where an empty side names the line it follows.
fn hunk_range(cursor: usize, count: usize) -> String {
    let start = if count == 0 { cursor } else { cursor + 1 };
    format!("{},{}", start, count)
}
