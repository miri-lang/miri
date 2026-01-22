// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR optimization passes and driver.
//!
//! This module provides an iterative optimization framework for MIR transformations.
//! Passes are run in sequence until a fixpoint is reached (no pass makes changes)
//! or a maximum iteration count is exceeded.
//!
//! # Available Passes
//!
//! | Pass | Description |
//! |------|-------------|
//! | [`SimplifyCfg`] | Removes empty blocks, threads unconditional jumps |
//! | [`ConstantPropagation`] | Replaces variables with known constant values |
//! | [`CopyPropagation`] | Eliminates redundant copy operations |
//! | [`DeadCodeElimination`] | Removes unused assignments and unreachable code |
//!
//! # Usage
//!
//! ```no_run
//! use miri::mir::optimization::optimize;
//! use miri::mir::Body;
//!
//! fn example(body: &mut Body) {
//!     optimize(body); // Runs all passes to fixpoint
//! }
//! ```

pub mod constant_propagation;
pub mod copy_propagation;
pub mod dead_code;
pub mod simplify_cfg;

use crate::mir::Body;
use constant_propagation::ConstantPropagation;
use copy_propagation::CopyPropagation;
use dead_code::DeadCodeElimination;
use simplify_cfg::SimplifyCfg;

/// Defines an optimization pass for MIR transformations.
///
/// Optimization passes are stateless transformations applied to MIR function bodies.
/// Each pass should focus on a single optimization strategy.
///
/// # Implementing a Pass
///
/// 1. Create a unit struct for your pass
/// 2. Implement `run()` to perform the transformation
/// 3. Return `true` from `run()` if any modifications were made
/// 4. Implement `name()` for debugging and logging
///
/// # Example
///
/// ```no_run
/// use miri::mir::Body;
/// use miri::mir::optimization::OptimizationPass;
///
/// pub struct MyOptimization;
///
/// impl OptimizationPass for MyOptimization {
///     fn run(&mut self, body: &mut Body) -> bool {
///         let mut changed = false;
///         // ... perform transformations ...
///         changed
///     }
///
///     fn name(&self) -> &'static str {
///         "My Optimization"
///     }
/// }
/// ```
pub trait OptimizationPass {
    /// Apply the optimization pass to the function body.
    ///
    /// Returns `true` if any modifications were made to the body.
    /// The optimizer uses this to determine when a fixpoint has been reached.
    fn run(&mut self, body: &mut Body) -> bool;

    /// Human-readable name for this pass, used in debugging and logging.
    fn name(&self) -> &'static str;
}

/// Run all optimization passes on the MIR body until a fixpoint is reached.
///
/// Passes are applied in a fixed order (SimplifyCfg → ConstantPropagation →
/// CopyPropagation → DeadCodeElimination) and the sequence repeats until
/// no pass makes changes or `MAX_ITERATIONS` (10) is reached.
///
/// # Arguments
///
/// * `body` - The MIR function body to optimize (mutated in place)
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
