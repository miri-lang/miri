// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The record a profiled run writes to `MIRI_PROFILE_OUT` as it exits.
//!
//! ```text
//! miri-profile 1
//! hz 1000
//! samples 1532
//! dropped 0
//! truncated 0
//! 1204 f3+212 f0+48
//! 310 0x1045f2c80:miri_rt_list_push+64 f3+96 f0+48
//! ```
//!
//! After the version line come the sampling rate, the samples taken, how
//! many of those were dropped for want of room in the buffer, and how many
//! were kept but belong to a stack past the most a record lists
//! ([`MAX_STACKS`]), so their stacks are not written. Then one line
//! per distinct stack: how many samples saw it, and its frames, the sampled
//! instruction first and each caller after it. A frame inside a function of
//! the build's table is `f<index>+<offset>`, the index the function has in the
//! build's line map and the offset in bytes from its first instruction, which
//! holds however the program was loaded. Any other frame is its address,
//! followed by `:<symbol>+<offset>` when the loader can name the symbol it
//! falls in. The lines are ordered by count, most first, then by frames.

use super::function_table::{FunctionTable, Location};
use std::collections::HashMap;
use std::fmt::Write;

/// The version the first line of a record names.
const FORMAT_VERSION: u32 = 1;

/// The most distinct stacks a record lists. A sample of a stack first seen
/// after this many is counted as truncated, so the exit-time aggregation is
/// bounded however varied the program's stacks are.
pub(crate) const MAX_STACKS: usize = 65_536;

/// One frame of a recorded stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Frame {
    /// An address inside a function of the build's table.
    Function { index: u32, offset: u32 },
    /// Any other address: the runtime, the C library, or code the table
    /// does not list.
    Other(usize),
}

impl Frame {
    /// The frame `address` is, placed against `table`.
    fn of(address: usize, table: &FunctionTable) -> Self {
        match table.locate(address) {
            Some(Location { index, offset }) => Self::Function { index, offset },
            None => Self::Other(address),
        }
    }
}

/// How many samples saw each distinct stack, up to a limit of stacks.
#[derive(Debug)]
pub(crate) struct StackCounts {
    counts: HashMap<Vec<Frame>, u64>,
    limit: usize,
    /// The samples of stacks first seen past the limit.
    truncated: u64,
}

impl Default for StackCounts {
    fn default() -> Self {
        Self::with_limit(MAX_STACKS)
    }
}

impl StackCounts {
    /// Counts listing at most `limit` distinct stacks.
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            counts: HashMap::new(),
            limit,
            truncated: 0,
        }
    }

    /// Count one sample of `addresses`, placed against `table`.
    pub(crate) fn add(&mut self, addresses: &[usize], table: &FunctionTable) {
        let stack: Vec<Frame> = addresses
            .iter()
            .map(|&address| Frame::of(address, table))
            .collect();
        if let Some(count) = self.counts.get_mut(&stack) {
            *count = count.saturating_add(1);
        } else if self.counts.len() < self.limit {
            self.counts.insert(stack, 1);
        } else {
            self.truncated = self.truncated.saturating_add(1);
        }
    }

    /// The samples counted, listed or truncated.
    pub(crate) fn samples(&self) -> u64 {
        self.counts
            .values()
            .fold(self.truncated, |sum, &count| sum.saturating_add(count))
    }
}

/// What a record says about the run beside its stacks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunSummary {
    /// The samples asked for per second of CPU time.
    pub hz: u32,
    /// The samples the run took but could not keep.
    pub dropped: u64,
}

/// The record of `stacks`. `symbolize` names the symbol an address outside
/// the table falls in, as `<symbol>+<offset>`, and is asked once per
/// address.
pub(crate) fn render(
    stacks: &StackCounts,
    summary: RunSummary,
    mut symbolize: impl FnMut(usize) -> Option<String>,
) -> String {
    let mut lines: Vec<(&Vec<Frame>, u64)> = stacks
        .counts
        .iter()
        .map(|(stack, &count)| (stack, count))
        .collect();
    lines.sort_by(|(left, left_count), (right, right_count)| {
        right_count.cmp(left_count).then_with(|| left.cmp(right))
    });
    let mut names: HashMap<usize, Option<String>> = HashMap::new();
    let mut text = String::new();
    // Writing into a `String` cannot fail.
    let _ = writeln!(text, "miri-profile {FORMAT_VERSION}");
    let _ = writeln!(text, "hz {}", summary.hz);
    let samples = stacks.samples().saturating_add(summary.dropped);
    let _ = writeln!(text, "samples {samples}");
    let _ = writeln!(text, "dropped {}", summary.dropped);
    let _ = writeln!(text, "truncated {}", stacks.truncated);
    for (stack, count) in lines {
        let _ = write!(text, "{count}");
        for frame in stack {
            match *frame {
                Frame::Function { index, offset } => {
                    let _ = write!(text, " f{index}+{offset}");
                }
                Frame::Other(address) => {
                    let _ = write!(text, " {address:#x}");
                    let name = names
                        .entry(address)
                        .or_insert_with(|| symbolize(address).map(|name| one_word(&name)));
                    if let Some(name) = name {
                        let _ = write!(text, ":{name}");
                    }
                }
            }
        }
        text.push('\n');
    }
    text
}

/// `name` with any whitespace or control character replaced, so it stays
/// one word of its line.
fn one_word(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::function_table::ProfileFunction;

    fn table() -> FunctionTable {
        FunctionTable::new(&[
            ProfileFunction {
                address: 0x1000,
                code_size: 0x100,
            },
            ProfileFunction {
                address: 0x2000,
                code_size: 0x100,
            },
        ])
    }

    #[test]
    fn stacks_are_counted_placed_and_listed_most_seen_first() {
        let table = table();
        let mut stacks = StackCounts::default();
        stacks.add(&[0x2010, 0x1020], &table);
        stacks.add(&[0x9000, 0x2010, 0x1020], &table);
        stacks.add(&[0x2010, 0x1020], &table);
        let record = render(
            &stacks,
            RunSummary {
                hz: 1000,
                dropped: 2,
            },
            |address| (address == 0x9000).then(|| "miri_rt_list_push+16".to_string()),
        );
        assert_eq!(
            record,
            "miri-profile 1\n\
             hz 1000\n\
             samples 5\n\
             dropped 2\n\
             truncated 0\n\
             2 f1+16 f0+32\n\
             1 0x9000:miri_rt_list_push+16 f1+16 f0+32\n"
        );
    }

    #[test]
    fn an_address_no_symbol_names_is_written_alone() {
        let table = table();
        let mut stacks = StackCounts::default();
        stacks.add(&[0x5000], &table);
        let record = render(
            &stacks,
            RunSummary {
                hz: 250,
                dropped: 0,
            },
            |_| None,
        );
        assert!(
            record.ends_with("samples 1\ndropped 0\ntruncated 0\n1 0x5000\n"),
            "{record}"
        );
    }

    #[test]
    fn a_symbol_with_a_space_stays_one_word() {
        let table = table();
        let mut stacks = StackCounts::default();
        stacks.add(&[0x5000], &table);
        let record = render(&stacks, RunSummary { hz: 1, dropped: 0 }, |_| {
            Some("a b\tc+1".to_string())
        });
        assert!(record.ends_with("1 0x5000:a_b_c+1\n"), "{record}");
    }

    #[test]
    fn ties_are_ordered_by_their_frames_so_the_record_is_reproducible() {
        let table = table();
        let mut stacks = StackCounts::default();
        stacks.add(&[0x2000], &table);
        stacks.add(&[0x1000], &table);
        let record = render(&stacks, RunSummary { hz: 1, dropped: 0 }, |_| None);
        assert!(record.ends_with("1 f0+0\n1 f1+0\n"), "{record}");
    }

    #[test]
    fn stacks_past_the_limit_are_counted_as_truncated_not_listed() {
        let table = table();
        let mut stacks = StackCounts::with_limit(2);
        stacks.add(&[0x1000], &table);
        stacks.add(&[0x1004], &table);
        stacks.add(&[0x1008], &table);
        stacks.add(&[0x100c], &table);
        stacks.add(&[0x1000], &table);
        let record = render(&stacks, RunSummary { hz: 1, dropped: 1 }, |_| None);
        assert!(
            record.contains("samples 6\ndropped 1\ntruncated 2\n2 f0+0\n1 f0+4\n"),
            "{record}"
        );
        assert!(!record.contains("f0+8"), "{record}");
    }
}
