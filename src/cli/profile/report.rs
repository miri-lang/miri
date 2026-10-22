// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Charging a record's samples to source lines and functions, and writing
//! the result for a person.

use super::line_table::{LineTable, SourceLine};
use super::record::{ProfileRecord, RecordedFrame};
use crate::cli::sanitize_for_terminal;
use crate::diagnostics::json::{JsonProfile, JsonProfileFunction, JsonProfileLine};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

/// The name a function outside the line map is charged under when the
/// loader named no symbol for it.
const UNKNOWN_FUNCTION: &str = "(unknown)";

/// How many samples a line holds.
#[derive(Debug, Default, Clone, Copy)]
struct LineSamples {
    own: u64,
    inclusive: u64,
}

/// A record's samples, charged.
#[derive(Debug, Default)]
pub(crate) struct Attribution {
    samples: u64,
    lines: HashMap<SourceLine, LineSamples>,
    functions: HashMap<String, u64>,
}

impl Attribution {
    /// Charge every stack of `record` against `table`: its first frame's line
    /// gets the samples as its own, or, outside any line, its function does;
    /// every distinct line in the stack gets them inclusively, the lines of
    /// the calls a frame's code was inlined through among them.
    pub(crate) fn of(record: &ProfileRecord, table: &LineTable) -> Self {
        let mut attribution = Self::default();
        let mut seen: HashSet<SourceLine> = HashSet::new();
        for stack in &record.stacks {
            // The counts come from a file; every total saturates.
            attribution.samples = attribution.samples.saturating_add(stack.count);
            let lines: Vec<&[SourceLine]> = stack
                .frames
                .iter()
                .map(|frame| lines_of(frame, table))
                .collect();
            let own = lines.first().and_then(|lines| lines.first()).copied();
            match (own, stack.frames.first()) {
                (Some(line), _) => {
                    let samples = attribution.lines.entry(line).or_default();
                    samples.own = samples.own.saturating_add(stack.count);
                }
                (None, Some(frame)) => {
                    let name = function_name(frame, table).to_string();
                    let samples = attribution.functions.entry(name).or_insert(0);
                    *samples = samples.saturating_add(stack.count);
                }
                (None, None) => {}
            }
            seen.clear();
            for &line in lines.into_iter().flatten() {
                if seen.insert(line) {
                    let samples = attribution.lines.entry(line).or_default();
                    samples.inclusive = samples.inclusive.saturating_add(stack.count);
                }
            }
        }
        attribution
    }
}

/// The source line `frame` falls on followed by the lines of the calls its
/// code was inlined through; empty outside any line.
fn lines_of<'t>(frame: &RecordedFrame, table: &'t LineTable) -> &'t [SourceLine] {
    match frame {
        RecordedFrame::Function { index, offset } => table.lines_at(*index, *offset),
        RecordedFrame::Other { .. } => &[],
    }
}

/// The function `frame` is charged under when it falls on no line.
fn function_name<'f>(frame: &'f RecordedFrame, table: &'f LineTable) -> &'f str {
    match frame {
        RecordedFrame::Function { index, .. } => table.symbol(*index).unwrap_or(UNKNOWN_FUNCTION),
        RecordedFrame::Other { symbol } => symbol.as_deref().unwrap_or(UNKNOWN_FUNCTION),
    }
}

/// What the report needs beside the samples: how many entries to keep, the
/// file names and a way to read a line's text.
pub(crate) struct ReportShape<'t> {
    pub top: usize,
    pub hz: u32,
    pub dropped: u64,
    pub truncated: u64,
    pub files: &'t [String],
}

/// The report of `attribution`: the `top` lines by self time, then by
/// inclusive time, and the `top` functions outside any line by self time.
/// `text_of` reads a line's source text.
pub(crate) fn report(
    attribution: &Attribution,
    shape: &ReportShape<'_>,
    mut text_of: impl FnMut(SourceLine) -> Option<String>,
) -> JsonProfile {
    let share = |samples: u64| samples as f64 / attribution.samples.max(1) as f64;
    let mut lines: Vec<(&SourceLine, &LineSamples)> = attribution.lines.iter().collect();
    lines.sort_by(|(left, left_samples), (right, right_samples)| {
        (right_samples.own, right_samples.inclusive)
            .cmp(&(left_samples.own, left_samples.inclusive))
            .then_with(|| left.cmp(right))
    });
    let lines = lines
        .into_iter()
        .take(shape.top)
        .map(|(&line, samples)| JsonProfileLine {
            path: shape.files.get(line.file).cloned().unwrap_or_default(),
            line: line.line,
            self_samples: samples.own,
            inclusive_samples: samples.inclusive,
            self_share: share(samples.own),
            inclusive_share: share(samples.inclusive),
            text: text_of(line),
        })
        .collect();
    let mut functions: Vec<(&String, &u64)> = attribution.functions.iter().collect();
    functions.sort_by(|(left, left_count), (right, right_count)| {
        right_count.cmp(left_count).then_with(|| left.cmp(right))
    });
    let runtime = functions
        .into_iter()
        .take(shape.top)
        .map(|(symbol, &samples)| JsonProfileFunction {
            symbol: symbol.clone(),
            self_samples: samples,
            self_share: share(samples),
        })
        .collect();
    JsonProfile {
        hz: shape.hz,
        samples: attribution.samples,
        dropped: shape.dropped,
        truncated: shape.truncated,
        lines,
        runtime,
    }
}

/// A share as a percentage, one decimal place.
fn percent(share: f64) -> String {
    format!("{:5.1}%", share * 100.0)
}

/// The report as a person reads it on a terminal. File names, source text
/// and symbols come from the program and the files it was built from, so a
/// control character in any of them is shown escaped rather than sent to the
/// terminal.
pub(crate) fn render(profile: &JsonProfile) -> String {
    let mut text = String::new();
    // Writing into a `String` cannot fail.
    let _ = writeln!(
        text,
        "profile: {} samples, at up to {} Hz of CPU time, {} dropped, {} truncated",
        profile.samples, profile.hz, profile.dropped, profile.truncated
    );
    let _ = writeln!(text, "\n  self   incl.  line");
    let locations: Vec<String> = profile
        .lines
        .iter()
        .map(|line| sanitize_for_terminal(&format!("{}:{}", line.path, line.line)))
        .collect();
    let width = locations
        .iter()
        .map(|location| location.chars().count())
        .max()
        .unwrap_or(0);
    for (line, location) in profile.lines.iter().zip(&locations) {
        let source = line.text.as_deref().map(str::trim).unwrap_or_default();
        let _ = writeln!(
            text,
            "{} {}  {location:width$}  {}",
            percent(line.self_share),
            percent(line.inclusive_share),
            sanitize_for_terminal(source)
        );
    }
    if !profile.runtime.is_empty() {
        let _ = writeln!(text, "\n  self   outside any source line");
        for function in &profile.runtime {
            let _ = writeln!(
                text,
                "{}  {}",
                percent(function.self_share),
                sanitize_for_terminal(&function.symbol)
            );
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "miri-lines 2\nfile 0 prog.mi\nfile 1 system/math.mi\n\
                       fn 0 miri.spin size=64\n  0 8 0:3\n  8 40 0:8\n  40 56 1:12 <- 0:8\n\
                       fn 1 miri$math.hash size=32\n  0 32 1:12\n\
                       fn 2 miri.$thunk size=8\n";

    fn attribution(record: &str) -> Attribution {
        let record = ProfileRecord::parse(record).unwrap_or_else(|e| panic!("{e}"));
        let table = LineTable::parse(MAP).unwrap_or_else(|e| panic!("{e}"));
        Attribution::of(&record, &table)
    }

    fn shaped(attribution: &Attribution, top: usize) -> JsonProfile {
        let files = ["prog.mi".to_string(), "system/math.mi".to_string()];
        let shape = ReportShape {
            top,
            hz: 1000,
            dropped: 0,
            truncated: 0,
            files: &files,
        };
        report(attribution, &shape, |line| {
            Some(format!("line {}", line.line))
        })
    }

    #[test]
    fn the_first_frame_holds_self_time_and_every_frame_inclusive_time() {
        let charged =
            attribution("miri-profile 1\nhz 1000\nsamples 4\ndropped 0\n3 f1+4 f0+10\n1 f0+12\n");
        let profile = shaped(&charged, 10);
        assert_eq!(profile.samples, 4);
        let first = &profile.lines[0];
        assert_eq!((first.path.as_str(), first.line), ("system/math.mi", 12));
        assert_eq!((first.self_samples, first.inclusive_samples), (3, 3));
        let caller = &profile.lines[1];
        assert_eq!((caller.path.as_str(), caller.line), ("prog.mi", 8));
        assert_eq!((caller.self_samples, caller.inclusive_samples), (1, 4));
        assert_eq!(caller.inclusive_share, 1.0);
        assert_eq!(caller.text.as_deref(), Some("line 8"));
    }

    /// Code inlined into the sampled function holds its own line's self
    /// time, and the line it was inlined at holds the time inclusively,
    /// once however often the stack names it.
    #[test]
    fn inlined_code_charges_the_line_it_was_inlined_at_inclusively() {
        let charged = attribution("miri-profile 1\nhz 1\n3 f0+44\n2 f0+44 f0+10\n");
        let profile = shaped(&charged, 10);
        let library = &profile.lines[0];
        assert_eq!(
            (library.path.as_str(), library.line),
            ("system/math.mi", 12)
        );
        assert_eq!((library.self_samples, library.inclusive_samples), (5, 5));
        let caller = &profile.lines[1];
        assert_eq!((caller.path.as_str(), caller.line), ("prog.mi", 8));
        assert_eq!((caller.self_samples, caller.inclusive_samples), (0, 5));
    }

    #[test]
    fn a_line_twice_in_one_stack_is_charged_once() {
        let charged = attribution("miri-profile 1\nhz 1\n2 f0+10 f0+20 f0+30\n");
        let profile = shaped(&charged, 10);
        assert_eq!(profile.lines.len(), 1);
        assert_eq!(profile.lines[0].inclusive_samples, 2);
    }

    #[test]
    fn time_outside_any_line_is_charged_to_its_function() {
        let charged = attribution(
            "miri-profile 1\nhz 1\n5 0x10:miri_rt_list_push+4 f0+10\n2 0x20 f0+10\n1 f2+0\n",
        );
        let profile = shaped(&charged, 10);
        let named: Vec<(&str, u64)> = profile
            .runtime
            .iter()
            .map(|function| (function.symbol.as_str(), function.self_samples))
            .collect();
        assert_eq!(
            named,
            vec![
                ("miri_rt_list_push", 5),
                ("(unknown)", 2),
                ("miri.$thunk", 1)
            ]
        );
        assert_eq!(profile.lines[0].self_samples, 0);
        assert_eq!(profile.lines[0].inclusive_samples, 7);
    }

    #[test]
    fn top_bounds_both_lists() {
        let charged =
            attribution("miri-profile 1\nhz 1\n3 f1+4\n2 f0+10\n1 f0+0\n4 0x10:a+0\n1 0x20:b+0\n");
        let profile = shaped(&charged, 1);
        assert_eq!(profile.lines.len(), 1);
        assert_eq!(profile.lines[0].line, 12);
        assert_eq!(profile.runtime.len(), 1);
        assert_eq!(profile.runtime[0].symbol, "a");
    }

    #[test]
    fn the_rendered_report_lists_each_line_with_its_shares_and_text() {
        let charged = attribution("miri-profile 1\nhz 1000\n3 f1+4 f0+10\n1 f0+12\n");
        let rendered = render(&shaped(&charged, 10));
        assert!(
            rendered.contains(" 75.0%  75.0%  system/math.mi:12  line 12"),
            "{rendered}"
        );
        assert!(rendered.contains(" 25.0% 100.0%  prog.mi:8"), "{rendered}");
    }

    #[test]
    fn counts_past_any_real_run_saturate_rather_than_overflow() {
        let charged = attribution(&format!(
            "miri-profile 1\nhz 1\n{max} f0+10\n{max} f0+12 f0+10\n{max} 0x10:a+0\n",
            max = u64::MAX
        ));
        let profile = shaped(&charged, 10);
        assert_eq!(profile.samples, u64::MAX);
        assert_eq!(profile.lines[0].self_samples, u64::MAX);
        assert_eq!(profile.lines[0].inclusive_samples, u64::MAX);
        assert_eq!(profile.runtime[0].self_samples, u64::MAX);
    }

    #[test]
    fn a_control_character_in_a_symbol_path_or_text_is_shown_escaped() {
        let profile = JsonProfile {
            hz: 1,
            samples: 1,
            dropped: 0,
            truncated: 2,
            lines: vec![JsonProfileLine {
                path: "a\u{1b}[2J.mi".to_string(),
                line: 1,
                self_samples: 1,
                inclusive_samples: 1,
                self_share: 1.0,
                inclusive_share: 1.0,
                text: Some("x = \"\u{1b}]0;title\u{7}\"".to_string()),
            }],
            runtime: vec![JsonProfileFunction {
                symbol: "f\u{1b}[31m".to_string(),
                self_samples: 1,
                self_share: 1.0,
            }],
        };
        let rendered = render(&profile);
        assert!(
            !rendered.contains('\u{1b}') && !rendered.contains('\u{7}'),
            "{rendered:?}"
        );
        assert!(rendered.contains("a\\u{1b}[2J.mi:1"), "{rendered}");
        assert!(rendered.contains("f\\u{1b}[31m"), "{rendered}");
        assert!(rendered.contains("2 truncated"), "{rendered}");
    }
}
