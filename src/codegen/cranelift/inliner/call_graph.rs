// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The direct references between the program's user functions, and an
//! order of their strongly connected components that puts every component
//! after the components it calls into.

use super::super::{user_function_id, TranslatedFunction};
use cranelift_codegen::ir::{FuncRef, Function, InstructionData, Opcode};
use std::collections::HashMap;

/// The direct references between the program's user functions, by each
/// function's position in the translated list.
pub(super) struct CallGraph {
    /// Each user function's position, by the index of its `FuncId`.
    positions: HashMap<u32, usize>,
    /// The distinct user functions each function refers to, ascending.
    callees: Vec<Vec<usize>>,
    /// How many `call` instructions name each function.
    call_sites: Vec<usize>,
}

impl CallGraph {
    pub fn of(functions: &[TranslatedFunction<'_>]) -> Self {
        let positions = functions
            .iter()
            .enumerate()
            .map(|(position, function)| (function.id.as_u32(), position))
            .collect();
        let mut graph = Self {
            positions,
            callees: Vec::with_capacity(functions.len()),
            call_sites: vec![0; functions.len()],
        };
        for translated in functions {
            graph.add_references_of(&translated.function);
        }
        graph
    }

    /// Record every user function `function` refers to, and every `call`
    /// instruction naming one.
    fn add_references_of(&mut self, function: &Function) {
        let mut callees: Vec<usize> = (function.dfg.ext_funcs.keys())
            .filter_map(|func_ref| self.position_of(function, func_ref))
            .collect();
        callees.sort_unstable();
        callees.dedup();
        self.callees.push(callees);
        for block in function.layout.blocks() {
            for inst in function.layout.block_insts(block) {
                if let InstructionData::Call {
                    opcode: Opcode::Call,
                    func_ref,
                    ..
                } = function.dfg.insts[inst]
                {
                    if let Some(position) = self.position_of(function, func_ref) {
                        self.call_sites[position] += 1;
                    }
                }
            }
        }
    }

    /// The position of the user function `func_ref` names in `function`,
    /// `None` for a runtime import or any other function.
    pub fn position_of(&self, function: &Function, func_ref: FuncRef) -> Option<usize> {
        let data = function.dfg.ext_funcs.get(func_ref)?;
        let id = user_function_id(function, &data.name)?;
        self.positions.get(&id.as_u32()).copied()
    }

    /// The distinct user functions the function at `index` refers to.
    pub fn callees(&self, index: usize) -> &[usize] {
        self.callees.get(index).map_or(&[], Vec::as_slice)
    }

    /// How many `call` instructions name the function at `index`.
    pub fn call_sites(&self, index: usize) -> usize {
        self.call_sites.get(index).copied().unwrap_or(0)
    }

    /// Whether the function at `index` refers to itself.
    pub fn is_self_recursive(&self, index: usize) -> bool {
        self.callees(index).binary_search(&index).is_ok()
    }

    /// For each function, how many other functions refer to it.
    pub fn distinct_callers(&self) -> Vec<usize> {
        let mut callers = vec![0; self.callees.len()];
        for (caller, callees) in self.callees.iter().enumerate() {
            for &callee in callees.iter().filter(|&&callee| callee != caller) {
                callers[callee] += 1;
            }
        }
        callers
    }

    /// The strongly connected components, each after every component it
    /// calls into.
    pub fn bottom_up_components(&self) -> Vec<Vec<usize>> {
        components(&self.callees)
    }
}

/// The strongly connected components of the graph whose edges `callees`
/// lists, each after every component it reaches, found by Tarjan's
/// algorithm from each node in turn.
pub(super) fn components(callees: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut search = ComponentSearch::new(callees.len());
    for root in 0..callees.len() {
        if search.discovered[root].is_none() {
            search.run_from(root, callees);
        }
    }
    search.components
}

/// The state of Tarjan's strongly connected components search, kept on
/// explicit stacks so a deep call chain cannot exhaust the thread's own.
struct ComponentSearch {
    discovered: Vec<Option<usize>>,
    low_link: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next_discovery: usize,
    components: Vec<Vec<usize>>,
}

impl ComponentSearch {
    fn new(count: usize) -> Self {
        Self {
            discovered: vec![None; count],
            low_link: vec![0; count],
            on_stack: vec![false; count],
            stack: Vec::new(),
            next_discovery: 0,
            components: Vec::new(),
        }
    }

    fn enter(&mut self, node: usize) {
        self.discovered[node] = Some(self.next_discovery);
        self.low_link[node] = self.next_discovery;
        self.next_discovery += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
    }

    /// Search every node reachable from `root`, emitting each component once
    /// all it reaches is emitted. An edge to a node past the graph is
    /// ignored.
    fn run_from(&mut self, root: usize, callees: &[Vec<usize>]) {
        self.enter(root);
        let mut path: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (node, ref mut next_edge)) = path.last_mut() {
            if let Some(&callee) = callees[node].get(*next_edge) {
                *next_edge += 1;
                match self.discovered.get(callee).copied().flatten() {
                    None if callee < callees.len() => {
                        self.enter(callee);
                        path.push((callee, 0));
                    }
                    Some(order) if self.on_stack[callee] => {
                        self.low_link[node] = self.low_link[node].min(order);
                    }
                    None | Some(_) => {}
                }
                continue;
            }
            path.pop();
            if let Some(&(parent, _)) = path.last() {
                self.low_link[parent] = self.low_link[parent].min(self.low_link[node]);
            }
            if Some(self.low_link[node]) == self.discovered[node] {
                self.emit_component(node);
            }
        }
    }

    /// Pop the component `root` heads off the stack.
    fn emit_component(&mut self, root: usize) {
        let mut component = Vec::new();
        while let Some(node) = self.stack.pop() {
            self.on_stack[node] = false;
            component.push(node);
            if node == root {
                break;
            }
        }
        component.reverse();
        self.components.push(component);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The position of the component holding `node`.
    fn rank(components: &[Vec<usize>], node: usize) -> usize {
        components
            .iter()
            .position(|component| component.contains(&node))
            .unwrap_or_else(|| panic!("{node} is in no component of {components:?}"))
    }

    #[test]
    fn a_cycle_is_one_component_after_what_it_calls() {
        let components = components(&[vec![1], vec![2], vec![0, 3], vec![]]);
        assert_eq!(components, vec![vec![3], vec![0, 1, 2]]);
    }

    #[test]
    fn a_self_loop_is_a_component_of_its_own() {
        let components = components(&[vec![0, 1], vec![]]);
        assert_eq!(components, vec![vec![1], vec![0]]);
    }

    #[test]
    fn a_diamond_puts_the_shared_callee_first_and_the_root_last() {
        let components = components(&[vec![1, 2], vec![3], vec![3], vec![]]);
        assert_eq!(components.len(), 4);
        assert_eq!(rank(&components, 3), 0);
        assert_eq!(rank(&components, 0), 3);
        assert!(rank(&components, 1) < rank(&components, 0));
        assert!(rank(&components, 2) < rank(&components, 0));
    }

    #[test]
    fn a_deep_chain_runs_on_the_explicit_stack() {
        const DEPTH: usize = 100_000;
        let callees: Vec<Vec<usize>> = (0..DEPTH)
            .map(|node| {
                if node + 1 < DEPTH {
                    vec![node + 1]
                } else {
                    Vec::new()
                }
            })
            .collect();
        let components = components(&callees);
        assert_eq!(components.len(), DEPTH);
        assert_eq!(components.first(), Some(&vec![DEPTH - 1]));
        assert_eq!(components.last(), Some(&vec![0]));
    }

    #[test]
    fn the_order_repeats_from_run_to_run() {
        let graph = [vec![2, 1], vec![3], vec![3], vec![], vec![0]];
        assert_eq!(components(&graph), components(&graph));
    }
}
