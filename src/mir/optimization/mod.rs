// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod constant_propagation;
pub mod copy_propagation;
pub mod dead_code;
pub mod simplify_cfg;

use crate::mir::Body;
use constant_propagation::ConstantPropagation;
use copy_propagation::CopyPropagation;
use dead_code::DeadCodeElimination;
use simplify_cfg::SimplifyCfg;

pub trait OptimizationPass {
    fn run(&mut self, body: &mut Body) -> bool;
    fn name(&self) -> &'static str;
}

pub fn optimize(body: &mut Body) {
    let mut passes: Vec<Box<dyn OptimizationPass>> = vec![
        Box::new(SimplifyCfg),
        Box::new(ConstantPropagation),
        Box::new(CopyPropagation),
        Box::new(DeadCodeElimination),
    ];

    let mut changed = true;
    let mut iteration = 0;
    const MAX_ITERATIONS: usize = 10;

    while changed && iteration < MAX_ITERATIONS {
        changed = false;
        iteration += 1;

        for pass in &mut passes {
            if pass.run(body) {
                changed = true;
            }
        }
    }
}
