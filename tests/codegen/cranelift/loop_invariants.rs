// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optimized build computes a value that does not change in an inner
//! loop outside that loop, even when the code a loop exits to is reached
//! before the loop's body in the dominator tree.

use super::slot_promotion::{function_text, listing_and_output};
use std::collections::{BTreeMap, BTreeSet};

/// Two nested loops whose inner body scales by `1.0 / (n as float)`, which
/// neither loop changes.
const NESTED_LOOPS: &str = r#"use system.io

fn kernel(n int) float
    var total = 0.0
    var i = 0
    while i < n
        var j = 0
        while j < n
            total += (j as float) * (1.0 / (n as float))
            j += 1
        i += 1
    total

fn main()
    println(f"{kernel(30)}")
"#;

/// Two nested loops whose inner body divides the outer counter by `n`, which
/// the inner loop changes neither of.
const OUTER_COUNTER_IN_INNER_LOOP: &str = r#"use system.io

fn kernel(n int) float
    var total = 0.0
    var i = 0
    while i < n
        var j = 0
        while j < n
            total += (i as float) / (n as float) + (j as float)
            j += 1
        i += 1
    total

fn main()
    println(f"{kernel(30)}")
"#;

/// The bits Cranelift prints for the `f64` constant 1.0.
const ONE: &str = "0x1.0000000000000p0";

/// The blocks of one CLIF function: each block's lines, its trailing
/// comment kept, and the blocks it branches to, in listing order.
struct Cfg {
    order: Vec<String>,
    lines: BTreeMap<String, Vec<String>>,
    successors: BTreeMap<String, Vec<String>>,
}

impl Cfg {
    fn of(function: &str) -> Self {
        let mut cfg = Self {
            order: Vec::new(),
            lines: BTreeMap::new(),
            successors: BTreeMap::new(),
        };
        let mut current: Option<String> = None;
        for line in function.lines() {
            if line.starts_with("block") {
                let name = block_names(line).next().unwrap();
                cfg.order.push(name.clone());
                cfg.lines.insert(name.clone(), Vec::new());
                cfg.successors.insert(name.clone(), Vec::new());
                current = Some(name);
                continue;
            }
            let Some(block) = &current else { continue };
            let code = line.split(';').next().unwrap().trim();
            cfg.successors
                .get_mut(block)
                .unwrap()
                .extend(block_names(code));
            cfg.lines
                .get_mut(block)
                .unwrap()
                .push(line.trim().to_string());
        }
        cfg
    }

    fn entry(&self) -> &str {
        &self.order[0]
    }

    fn predecessors(&self, block: &str) -> Vec<&String> {
        (self.successors.iter())
            .filter(|(_, targets)| targets.iter().any(|target| target == block))
            .map(|(predecessor, _)| predecessor)
            .collect()
    }

    /// The blocks reachable from the entry in reverse postorder, and every
    /// edge a depth-first walk takes back to a block still on its path.
    fn walk(&self) -> (Vec<String>, Vec<(String, String)>) {
        let mut postorder = Vec::new();
        let mut retreating = Vec::new();
        let mut on_path = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let entry = self.entry().to_string();
        let mut stack = vec![(entry.clone(), 0usize)];
        on_path.insert(entry.clone());
        seen.insert(entry);
        while let Some((block, next)) = stack.pop() {
            let Some(successor) = self.successors[&block].get(next).cloned() else {
                on_path.remove(&block);
                postorder.push(block);
                continue;
            };
            stack.push((block.clone(), next + 1));
            if on_path.contains(&successor) {
                retreating.push((block, successor));
            } else if seen.insert(successor.clone()) {
                on_path.insert(successor.clone());
                stack.push((successor, 0));
            }
        }
        postorder.reverse();
        (postorder, retreating)
    }

    /// The dominators of every block of `order`, a reverse postorder of the
    /// reachable blocks.
    fn dominators(&self, order: &[String]) -> BTreeMap<String, BTreeSet<String>> {
        let all: BTreeSet<String> = order.iter().cloned().collect();
        let mut dominators: BTreeMap<String, BTreeSet<String>> = (order.iter())
            .map(|block| (block.clone(), all.clone()))
            .collect();
        let entry = self.entry().to_string();
        dominators.insert(entry.clone(), BTreeSet::from([entry.clone()]));
        let mut changed = true;
        while changed {
            changed = false;
            for block in order.iter().filter(|block| **block != entry) {
                let mut meet: Option<BTreeSet<String>> = None;
                for predecessor in self.predecessors(block) {
                    let Some(of_predecessor) = dominators.get(predecessor) else {
                        continue;
                    };
                    meet = Some(match meet {
                        None => of_predecessor.clone(),
                        Some(so_far) => &so_far & of_predecessor,
                    });
                }
                let mut next = meet.unwrap_or_default();
                next.insert(block.clone());
                if dominators[block] != next {
                    dominators.insert(block.clone(), next);
                    changed = true;
                }
            }
        }
        dominators
    }
}

/// The `block<N>` names `text` mentions.
fn block_names(text: &str) -> impl Iterator<Item = String> + '_ {
    text.match_indices("block").filter_map(|(start, _)| {
        let digits: String = text[start + "block".len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        (!digits.is_empty()).then(|| format!("block{digits}"))
    })
}

/// A natural loop: its header and every block of its body, the header
/// included.
#[derive(Debug, Clone)]
struct Loop {
    header: String,
    body: BTreeSet<String>,
}

/// The natural loops of `cfg`, one per header, from its back edges: the
/// edges whose target dominates their source. Every edge a depth-first walk
/// retreats along must be one, so an irreducible graph fails here.
fn natural_loops(cfg: &Cfg) -> Vec<Loop> {
    let (order, retreating) = cfg.walk();
    let dominators = cfg.dominators(&order);
    let mut loops: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (latch, header) in retreating {
        assert!(
            dominators[&latch].contains(&header),
            "the edge {latch} -> {header} retreats without {header} dominating {latch}: \
             the control flow graph is irreducible"
        );
        let body = loops.entry(header.clone()).or_default();
        body.insert(header.clone());
        let mut work = vec![latch];
        while let Some(block) = work.pop() {
            if body.insert(block.clone()) {
                work.extend(cfg.predecessors(&block).into_iter().cloned());
            }
        }
    }
    (loops.into_iter())
        .map(|(header, body)| Loop { header, body })
        .collect()
}

/// The innermost loop of `loops` holding another's header, for each loop
/// that has one: the loop it is nested in.
fn parents(loops: &[Loop]) -> BTreeMap<String, String> {
    (loops.iter())
        .filter_map(|inner| {
            let parent = (loops.iter())
                .filter(|outer| outer.header != inner.header && outer.body.contains(&inner.header))
                .min_by_key(|outer| outer.body.len())?;
            Some((inner.header.clone(), parent.header.clone()))
        })
        .collect()
}

/// The two values the header of `inner` compares: its counter and the
/// bound it runs to.
fn compared_by(cfg: &Cfg, inner: &Loop) -> (String, String) {
    cfg.lines[&inner.header]
        .iter()
        .find_map(|line| {
            let (_, compare) = line.split_once(" = icmp ")?;
            let operands = compare.split(';').next()?;
            let (left, right) = operands.split_once(", ")?;
            let counter = left.split_whitespace().last()?;
            Some((counter.to_string(), right.trim().to_string()))
        })
        .unwrap_or_else(|| panic!("the header {} compares nothing", inner.header))
}

/// The value the header of `inner` compares its counter with.
fn bound_of(cfg: &Cfg, inner: &Loop) -> String {
    compared_by(cfg, inner).1
}

/// What a build of `source` at opt level 3 prints, checked to be what the
/// unoptimized build prints, and its kernel with the kernel's control flow
/// graph and its inner loop, checked to sit inside the outer one.
fn nested_kernel(source: &str) -> (String, String, Cfg, Loop) {
    let (listing, output) = listing_and_output(source, "3");
    let (_, unoptimized) = listing_and_output(source, "0");
    assert_eq!(output, unoptimized);
    let kernel = function_text(&listing, "miri.kernel").to_string();
    let cfg = Cfg::of(&kernel);
    let loops = natural_loops(&cfg);
    let nesting = parents(&loops);
    let (inner_header, outer_header) = nesting
        .iter()
        .next()
        .unwrap_or_else(|| panic!("no loop is nested in another:\n{kernel}"));
    let inner = (loops.iter())
        .find(|candidate| candidate.header == *inner_header)
        .unwrap()
        .clone();
    let outer = (loops.iter())
        .find(|candidate| candidate.header == *outer_header)
        .unwrap();
    assert!(
        inner.body.is_subset(&outer.body),
        "the inner loop at {inner_header} is not inside the outer one at {outer_header}:\n{kernel}"
    );
    (output, kernel, cfg, inner)
}

/// The blocks of `cfg` holding a line `is_it` admits.
fn blocks_holding(cfg: &Cfg, is_it: impl Fn(&str) -> bool) -> BTreeSet<String> {
    (cfg.lines.iter())
        .filter(|(_, lines)| lines.iter().any(|line| is_it(line)))
        .map(|(block, _)| block.clone())
        .collect()
}

#[test]
fn a_value_no_loop_changes_is_computed_outside_the_inner_loop() {
    let (output, kernel, cfg, inner) = nested_kernel(NESTED_LOOPS);
    assert_eq!(output, "434.9999999999999\n");
    let inner = &inner;
    let bound = bound_of(&cfg, inner);
    let conversions = blocks_holding(&cfg, |line| {
        let code = line.split(';').next().unwrap().trim_end();
        code.contains(" = fcvt_from_sint.f64 ") && code.ends_with(&format!(" {bound}"))
    });
    let reciprocals = blocks_holding(&cfg, |line| line.contains(" = fdiv ") && line.contains(ONE));
    for (what, holders) in [
        ("`n as float`", &conversions),
        ("`1.0 / (n as float)`", &reciprocals),
    ] {
        assert!(
            holders.iter().any(|block| !inner.body.contains(block)),
            "{what} is computed nowhere outside the inner loop:\n{kernel}"
        );
        let inside: Vec<&String> = holders.intersection(&inner.body).collect();
        assert!(
            inside.is_empty(),
            "{what} is computed in {inside:?}, inside the inner loop:\n{kernel}"
        );
    }
}

/// A value computed from the outer loop's counter, which the inner loop
/// hands back unchanged on every iteration, is computed once per outer
/// iteration, outside the inner loop: the counter's conversion and the
/// division by `n`.
#[test]
fn a_value_of_the_outer_counter_is_computed_outside_the_inner_loop() {
    let (_, kernel, cfg, inner) = nested_kernel(OUTER_COUNTER_IN_INNER_LOOP);
    let (inner_counter, _) = compared_by(&cfg, &inner);
    let conversions_of_others = blocks_holding(&cfg, |line| {
        let code = line.split(';').next().unwrap().trim_end();
        code.contains(" = fcvt_from_sint.f64 ") && !code.ends_with(&format!(" {inner_counter}"))
    });
    let divisions = blocks_holding(&cfg, |line| line.contains(" = fdiv "));
    for (what, holders) in [
        ("`i as float`", &conversions_of_others),
        ("`(i as float) / (n as float)`", &divisions),
    ] {
        assert!(
            holders.iter().any(|block| !inner.body.contains(block)),
            "{what} is computed nowhere outside the inner loop:\n{kernel}"
        );
        let inside: Vec<&String> = holders.intersection(&inner.body).collect();
        assert!(
            inside.is_empty(),
            "{what} is computed in {inside:?}, inside the inner loop:\n{kernel}"
        );
    }
}
