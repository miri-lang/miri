// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How long each candidate's body is kept.
//!
//! A call of a candidate can reach a caller in two ways: the caller calls
//! it itself, or the caller inlines another candidate whose body still
//! holds the call. So a candidate is needed while one of its direct callers
//! is still to be visited, or while a live candidate's body holds a call of
//! it. Once neither holds it is dropped, and the calls its own body holds
//! are released, which can drop the candidates they name in turn. A
//! function that is no candidate is never copied, so the calls it holds
//! keep nothing alive once it has been visited.

/// What keeps each function's candidate alive, by position.
pub(super) struct Lifetimes {
    /// How many of each function's direct callers, itself left out, are
    /// still to be visited.
    callers_left: Vec<usize>,
    /// How many live candidates' bodies hold a call of each function.
    holders: Vec<usize>,
    /// The functions each live candidate's body calls, each once.
    held: Vec<Vec<usize>>,
    /// Whether each function has a live candidate.
    live: Vec<bool>,
}

impl Lifetimes {
    /// The lifetimes of functions with `callers` distinct direct callers
    /// each, none of them yet a candidate.
    pub(super) fn new(callers: Vec<usize>) -> Self {
        let count = callers.len();
        Self {
            callers_left: callers,
            holders: vec![0; count],
            held: vec![Vec::new(); count],
            live: vec![false; count],
        }
    }

    /// Record the candidate at `position`, whose body calls the functions
    /// `calls` (itself left out).
    pub(super) fn admit(&mut self, position: usize, mut calls: Vec<usize>) {
        calls.sort_unstable();
        calls.dedup();
        calls.retain(|&callee| callee != position && callee < self.live.len());
        for &callee in &calls {
            self.holders[callee] += 1;
        }
        self.held[position] = calls;
        self.live[position] = true;
    }

    /// Count the function at `position` as visited by each of its direct
    /// `callees`, returning the candidates no longer needed, in the order
    /// they are dropped.
    pub(super) fn visited(&mut self, position: usize, callees: &[usize]) -> Vec<usize> {
        let mut work = vec![position];
        for &callee in callees.iter().filter(|&&callee| callee != position) {
            if let Some(left) = self.callers_left.get_mut(callee) {
                *left = left.saturating_sub(1);
                work.push(callee);
            }
        }
        let mut dropped = Vec::new();
        while let Some(candidate) = work.pop() {
            if !self.live[candidate]
                || self.callers_left[candidate] != 0
                || self.holders[candidate] != 0
            {
                continue;
            }
            self.live[candidate] = false;
            dropped.push(candidate);
            for callee in std::mem::take(&mut self.held[candidate]) {
                self.holders[callee] = self.holders[callee].saturating_sub(1);
                work.push(callee);
            }
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_candidate_is_dropped_once_its_last_caller_is_visited() {
        // 0 is called by 1 and 2.
        let mut lifetimes = Lifetimes::new(vec![2, 0, 0]);
        lifetimes.admit(0, Vec::new());
        assert!(lifetimes.visited(0, &[]).is_empty());
        assert!(lifetimes.visited(1, &[0]).is_empty());
        assert_eq!(lifetimes.visited(2, &[0]), vec![0]);
    }

    #[test]
    fn a_call_held_only_by_a_function_that_is_no_candidate_keeps_nothing() {
        // 0 is called by 1, which is no candidate, and 1 by 2.
        let mut lifetimes = Lifetimes::new(vec![1, 1, 0]);
        lifetimes.admit(0, Vec::new());
        assert!(lifetimes.visited(0, &[]).is_empty());
        assert_eq!(lifetimes.visited(1, &[0]), vec![0]);
        assert!(lifetimes.visited(2, &[1]).is_empty());
    }

    #[test]
    fn a_candidate_whose_body_holds_a_call_keeps_its_callee_until_it_is_dropped() {
        // 0 is called by 1, a candidate that keeps the call, and 1 by 2.
        let mut lifetimes = Lifetimes::new(vec![1, 1, 0]);
        lifetimes.admit(0, Vec::new());
        assert!(lifetimes.visited(0, &[]).is_empty());
        lifetimes.admit(1, vec![0, 0]);
        assert!(lifetimes.visited(1, &[0]).is_empty());
        assert_eq!(lifetimes.visited(2, &[1]), vec![1, 0]);
    }

    #[test]
    fn a_candidate_that_inlined_its_callee_releases_it() {
        // As above, but 1 inlined its call of 0 and holds none.
        let mut lifetimes = Lifetimes::new(vec![1, 1, 0]);
        lifetimes.admit(0, Vec::new());
        lifetimes.visited(0, &[]);
        lifetimes.admit(1, Vec::new());
        assert_eq!(lifetimes.visited(1, &[0]), vec![0]);
        assert_eq!(lifetimes.visited(2, &[1]), vec![1]);
    }
}
