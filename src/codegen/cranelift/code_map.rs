// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which source location the code at the head of a machine block is charged
//! to when Cranelift gave it none.
//!
//! Cranelift's optimizer moves a loop-invariant constant out of a loop and
//! re-creates a copy of it in front of each use inside the loop, and the copy
//! carries no source location. A loop block can then begin with code no run
//! of the code map covers, which a line table would charge to the run before
//! it in layout: the code of an unrelated block, often the function's return.
//! The copy feeds the located instruction after it in its block, so the code
//! a block begins with is charged to the block's first run. Unlocated code in
//! the middle of a block is left to the run before it.
//!
//! The limit of the rule: the head inherits the whole location of that first
//! run, the chain of calls it was inlined through included. A constant
//! re-created in front of inlined code is charged to the inlined function and
//! its call chain even when the constant was written in the caller.

use crate::codegen::CodeRange;

/// `ranges`, sorted by start, with a run added for every block, among the
/// blocks starting at `block_starts` in layout order, that begins with code
/// no run covers: from the block's start to the block's first run, charged
/// as that run is. A block with no run of its own is left as it is.
pub(super) fn charge_block_heads(
    mut ranges: Vec<CodeRange>,
    block_starts: &[u32],
) -> Vec<CodeRange> {
    let ends = (block_starts.iter().skip(1).copied().map(Some)).chain(std::iter::once(None));
    let heads: Vec<CodeRange> = (block_starts.iter().copied().zip(ends))
        .filter_map(|(start, end)| {
            let after = ranges.partition_point(|range| range.start <= start);
            let covered = after
                .checked_sub(1)
                .and_then(|before| ranges.get(before))
                .is_some_and(|before| before.end > start);
            let first = ranges.get(after)?;
            let in_block = end.is_none_or(|end| first.start < end);
            (!covered && in_block).then(|| CodeRange {
                start,
                end: first.start,
                ..first.clone()
            })
        })
        .collect();
    ranges.extend(heads);
    ranges.sort_by_key(|range| range.start);
    ranges
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

    #[test]
    fn unlocated_code_heading_a_block_is_charged_to_the_blocks_first_run() {
        let ranges = vec![run(0, 8, 1), run(12, 20, 2)];
        let charged = charge_block_heads(ranges, &[0, 8]);
        assert_eq!(spans(&charged), vec![(0, 8, 1), (8, 12, 2), (12, 20, 2)]);
    }

    #[test]
    fn unlocated_code_inside_a_block_is_left_to_the_run_before_it() {
        let ranges = vec![run(0, 8, 1), run(12, 20, 2)];
        let charged = charge_block_heads(ranges.clone(), &[0]);
        assert_eq!(charged, ranges);
    }

    #[test]
    fn a_block_whose_head_a_run_covers_is_left_alone() {
        let ranges = vec![run(0, 8, 1), run(16, 20, 2)];
        let charged = charge_block_heads(ranges.clone(), &[0, 4, 16]);
        assert_eq!(charged, ranges);
    }

    #[test]
    fn a_block_with_no_run_of_its_own_is_left_alone() {
        let ranges = vec![run(0, 8, 1), run(16, 20, 2)];
        let charged = charge_block_heads(ranges.clone(), &[0, 8, 16]);
        assert_eq!(charged, ranges);
    }

    #[test]
    fn the_last_block_reaches_the_end_of_the_function() {
        let ranges = vec![run(0, 8, 1), run(12, 20, 2)];
        let charged = charge_block_heads(ranges, &[0, 10]);
        assert_eq!(spans(&charged), vec![(0, 8, 1), (10, 12, 2), (12, 20, 2)]);
    }

    #[test]
    fn a_function_with_no_runs_gets_none() {
        assert!(charge_block_heads(Vec::new(), &[0, 8]).is_empty());
    }

    #[test]
    fn without_a_block_layout_the_runs_are_unchanged() {
        let ranges = vec![run(4, 8, 1), run(12, 20, 2)];
        assert_eq!(charge_block_heads(ranges.clone(), &[]), ranges);
    }

    #[test]
    fn a_head_carries_the_inlining_chain_of_the_run_it_is_charged_to() {
        let mut inlined = run(12, 20, 2);
        inlined.inlined_at = vec![SourcePosition {
            source: SourceId::PROGRAM,
            offset: 40,
        }];
        let charged = charge_block_heads(vec![run(0, 8, 1), inlined.clone()], &[0, 8]);
        let head = CodeRange {
            start: 8,
            end: 12,
            ..inlined.clone()
        };
        assert_eq!(charged, vec![run(0, 8, 1), head, inlined]);
    }

    #[test]
    fn adjacent_blocks_each_get_their_own_head() {
        let ranges = vec![run(0, 8, 1), run(12, 16, 2), run(20, 24, 3)];
        let charged = charge_block_heads(ranges, &[0, 8, 16]);
        assert_eq!(
            spans(&charged),
            vec![(0, 8, 1), (8, 12, 2), (12, 16, 2), (16, 20, 3), (20, 24, 3)]
        );
    }
}
