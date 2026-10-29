// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which source location the code Cranelift gave none is charged to, so
//! that every byte of a function compiled from source lies on a line.
//!
//! Cranelift leaves three kinds of code unlocated. The prologue, which sets
//! up the frame, is emitted under no location at all; it is charged to the
//! function's signature, where its epilogue is already charged through the
//! function's return. The optimizer creates new instructions while it
//! simplifies, such as a division by a constant rewritten as a shift, and
//! moves a constant out of a loop and re-creates a copy of it in front of
//! each use inside the loop; neither carries a location. Such code is placed
//! in front of the instruction it feeds, so it is charged to the next run of
//! its own block. A block with no run after the code, which ends with
//! values handed to the block it jumps to or holds no located code at all,
//! is charged to the run before it, as a debugger's line table charges it.
//!
//! The limit of the rule: unlocated code inherits the whole location of the
//! run it is charged to, the chain of calls it was inlined through included,
//! and a constant handed to another block is charged to the code before the
//! jump rather than to the code in the block that reads it.

use crate::codegen::{CodeRange, SourcePosition};

/// `ranges`, sorted by start and not overlapping, extended to cover all
/// `size` bytes of the function laid out in the blocks starting at
/// `block_starts`: the code before the first run charged to `signature`,
/// or to that run when the function has none, and every other byte no run
/// covers charged to the next run of its block, or else to the run before
/// it. A function with no runs is charged whole to `signature`.
pub(super) fn cover_function(
    ranges: Vec<CodeRange>,
    block_starts: &[u32],
    size: u32,
    signature: Option<SourcePosition>,
) -> Vec<CodeRange> {
    let at_signature = signature.map(|at| CodeRange {
        start: 0,
        end: 0,
        source: at.source,
        source_offset: at.offset,
        inlined_at: Vec::new(),
    });
    let Some(first) = ranges.first() else {
        return at_signature
            .and_then(|whole| charged_as(0, size, &whole))
            .into_iter()
            .collect();
    };
    let mut covered = Vec::with_capacity(2 * ranges.len() + 1);
    covered.extend(charged_as(
        0,
        first.start,
        at_signature.as_ref().unwrap_or(first),
    ));
    let mut runs = ranges.into_iter().peekable();
    while let Some(run) = runs.next() {
        let next = runs.peek();
        let end = next.map_or(size, |next| next.start);
        let split = next.map_or(end, |next| {
            block_holding(block_starts, next.start)
                .max(run.end)
                .min(end)
        });
        let before = charged_as(run.end, split, &run);
        let after = next.and_then(|next| charged_as(split, end, next));
        covered.push(run);
        covered.extend(before);
        covered.extend(after);
    }
    covered
}

/// The bytes `start..end`, charged as `run` is; `None` when they are none.
fn charged_as(start: u32, end: u32, run: &CodeRange) -> Option<CodeRange> {
    (start < end).then(|| CodeRange {
        start,
        end,
        ..run.clone()
    })
}

/// The start of the block holding `offset`: the last of `block_starts`, in
/// layout order, at or before it, and the function's start when there is
/// none.
fn block_holding(block_starts: &[u32], offset: u32) -> u32 {
    let after = block_starts.partition_point(|&start| start <= offset);
    after
        .checked_sub(1)
        .and_then(|index| block_starts.get(index))
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::SourcePosition;
    use crate::error::SourceId;

    fn run(start: u32, end: u32, source_offset: u32) -> CodeRange {
        CodeRange {
            start,
            end,
            source: SourceId::PROGRAM,
            source_offset,
            inlined_at: Vec::new(),
        }
    }

    fn spans(ranges: &[CodeRange]) -> Vec<(u32, u32, u32)> {
        ranges
            .iter()
            .map(|range| (range.start, range.end, range.source_offset))
            .collect()
    }

    const SIGNATURE: SourcePosition = SourcePosition {
        source: SourceId::PROGRAM,
        offset: 90,
    };

    #[test]
    fn the_code_before_the_first_run_is_charged_to_the_signature() {
        let covered = cover_function(vec![run(8, 20, 1)], &[0], 20, Some(SIGNATURE));
        assert_eq!(spans(&covered), vec![(0, 8, 90), (8, 20, 1)]);
    }

    #[test]
    fn without_a_signature_the_code_before_the_first_run_is_charged_to_it() {
        let covered = cover_function(vec![run(8, 20, 1)], &[0], 20, None);
        assert_eq!(spans(&covered), vec![(0, 8, 1), (8, 20, 1)]);
    }

    #[test]
    fn unlocated_code_inside_a_block_is_charged_to_the_run_after_it() {
        let covered = cover_function(vec![run(0, 8, 1), run(12, 20, 2)], &[0], 20, None);
        assert_eq!(spans(&covered), vec![(0, 8, 1), (8, 12, 2), (12, 20, 2)]);
    }

    #[test]
    fn unlocated_code_heading_a_block_is_charged_to_the_blocks_first_run() {
        let covered = cover_function(vec![run(0, 8, 1), run(12, 20, 2)], &[0, 8], 20, None);
        assert_eq!(spans(&covered), vec![(0, 8, 1), (8, 12, 2), (12, 20, 2)]);
    }

    #[test]
    fn unlocated_code_ending_a_block_is_charged_to_the_run_before_it() {
        let covered = cover_function(vec![run(0, 8, 1), run(12, 20, 2)], &[0, 12], 20, None);
        assert_eq!(spans(&covered), vec![(0, 8, 1), (8, 12, 1), (12, 20, 2)]);
    }

    #[test]
    fn a_block_with_no_run_of_its_own_is_charged_to_the_run_before_it() {
        let covered = cover_function(vec![run(0, 8, 1), run(16, 20, 2)], &[0, 8, 16], 20, None);
        assert_eq!(spans(&covered), vec![(0, 8, 1), (8, 16, 1), (16, 20, 2)]);
    }

    #[test]
    fn a_hole_reaching_into_the_next_runs_block_is_split_at_that_block() {
        let covered = cover_function(vec![run(0, 8, 1), run(20, 24, 2)], &[0, 12], 24, None);
        assert_eq!(
            spans(&covered),
            vec![(0, 8, 1), (8, 12, 1), (12, 20, 2), (20, 24, 2)]
        );
    }

    #[test]
    fn the_code_after_the_last_run_is_charged_to_it() {
        let covered = cover_function(vec![run(0, 8, 1)], &[0], 16, None);
        assert_eq!(spans(&covered), vec![(0, 8, 1), (8, 16, 1)]);
    }

    #[test]
    fn a_covered_function_is_unchanged() {
        let ranges = vec![run(0, 8, 1), run(8, 20, 2)];
        assert_eq!(
            cover_function(ranges.clone(), &[0, 8], 20, Some(SIGNATURE)),
            ranges
        );
    }

    #[test]
    fn without_a_block_layout_the_function_is_one_block() {
        let covered = cover_function(vec![run(4, 8, 1), run(12, 20, 2)], &[], 20, None);
        assert_eq!(
            spans(&covered),
            vec![(0, 4, 1), (4, 8, 1), (8, 12, 2), (12, 20, 2)]
        );
    }

    #[test]
    fn a_function_with_no_runs_is_charged_whole_to_its_signature() {
        let covered = cover_function(Vec::new(), &[0, 8], 16, Some(SIGNATURE));
        assert_eq!(spans(&covered), vec![(0, 16, 90)]);
    }

    #[test]
    fn a_function_with_no_runs_and_no_signature_gets_none() {
        assert!(cover_function(Vec::new(), &[0, 8], 16, None).is_empty());
    }

    #[test]
    fn a_fill_carries_the_inlining_chain_of_the_run_it_is_charged_to() {
        let mut inlined = run(12, 20, 2);
        inlined.inlined_at = vec![SourcePosition {
            source: SourceId::PROGRAM,
            offset: 40,
        }];
        let covered = cover_function(vec![run(0, 8, 1), inlined.clone()], &[0, 8], 20, None);
        let head = CodeRange {
            start: 8,
            end: 12,
            ..inlined.clone()
        };
        assert_eq!(covered, vec![run(0, 8, 1), head, inlined]);
    }
}
